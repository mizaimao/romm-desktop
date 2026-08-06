//! Stage 5a — scan local save/state files into `ClientSaveState` candidates.
//!
//! Layout assumed is RetroArch's "sort by core" option:
//!
//! ```text
//! <root>/saves/<Core Display Name>/<rom basename>.srm
//! <root>/states/<Core Display Name>/<rom basename>.state[N][.auto]
//! ```
//!
//! Two things here are load-bearing and easy to get wrong:
//!
//! * **Slot names must be stable and non-null.** Saves pair on
//!   `(rom_id, slot)`, and a null slot is treated as an archival upload that
//!   *always* negotiates as `upload` — so unstable slots accumulate duplicates
//!   without bound.
//! * **`rom_id` resolution must be platform-scoped.** This library has
//!   `nes`+`famicom`, `snes`+`sfc` and `arcade`+`mame` as separate platforms
//!   with overlapping filenames; `airattck.zip` genuinely exists in both
//!   `arcade` and `mame`. Ambiguity is reported, never guessed.

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::cache::Cache;
use crate::coremap::CoreMap;
use crate::savehash;

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Kind {
    Save,
    State,
}

// Several fields exist for the negotiate payload (stage 5b) rather than for
// today's `scan` output; ClientSaveState needs size and content_hash.
#[allow(dead_code)]
#[derive(Debug)]
pub struct Candidate {
    pub path: PathBuf,
    pub kind: Kind,
    /// Core display name as it appeared in the directory, e.g. `"melonDS DS"`.
    pub core_dir: String,
    /// Resolved libretro core stem, e.g. `"melondsds"`.
    pub core: Option<String>,
    /// ROM basename the emulator derived the save name from.
    pub rom_base: String,
    pub slot: String,
    pub size: u64,
    pub content_hash: String,
    pub resolution: Resolution,
    /// False when another file claims the same `(rom_id, slot)` and comes from
    /// a more current core. Saves pair on `(rom_id, slot)`, so syncing both
    /// would have them overwrite each other on every run.
    pub canonical: bool,
    /// Set on non-canonical entries: the core that won.
    pub superseded_by: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug)]
pub enum Resolution {
    /// Exactly one ROM matched, on one platform.
    Resolved { rom_id: i64, platform: String, fs_name: String },
    /// The same filename exists on more than one candidate platform.
    Ambiguous(Vec<(i64, String, String)>),
    /// No ROM matched — orphan save, or the ROM is not on the server.
    Unmatched,
    /// The core directory did not map to a known libretro core.
    UnknownCore,
}

/// Normalise a core display name for matching: lowercase, alphanumerics only.
/// RetroArch's desktop display names differ from ES-DE's labels — `"MAME"` vs
/// `"MAME - Current"`, `"melonDS DS"` vs `melondsds` — so compare on a reduced
/// form rather than exact strings.
fn normalise(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Files that live in the save tree but are not saves.
fn is_ignorable(rel: &Path, file_name: &str) -> bool {
    // MAME writes its own config under <core>/mame/cfg/.
    if rel.components().any(|c| c.as_os_str() == "cfg") {
        return true;
    }
    // PCSX2 memory-card internals.
    if file_name.starts_with("_pcsx2") || file_name.starts_with('.') {
        return true;
    }
    matches!(
        Path::new(file_name)
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("cfg" | "ini" | "txt" | "ico" | "icn" | "sys")
    )
}

/// Does this filename belong in the `states` tree rather than `saves`?
///
/// Shares its rule with [`split_slot`] below, which is the authority on the
/// naming: anything carrying `.state` is a save state, everything else
/// (`.srm`, `.sav`) is a battery save. A download filed under the wrong one is
/// invisible to the next scan.
pub fn is_state_name(file_name: &str) -> bool {
    file_name.to_ascii_lowercase().contains(".state")
}

/// Split a save filename into (rom basename, slot).
///
/// RetroArch conventions, plus what this collection actually contains:
/// `.srm`/`.sav` are battery saves; `.state` is manual slot 0; `.stateN` is
/// slot N; `.state.auto` is the autosave slot. Opera writes `<name>.0.srm`
/// under `per_game/`.
fn split_slot(file_name: &str) -> Option<(String, String)> {
    let lower = file_name.to_ascii_lowercase();

    if let Some(stem) = lower.strip_suffix(".state.auto") {
        return Some((file_name[..stem.len()].to_owned(), "auto".into()));
    }
    if let Some(pos) = lower.rfind(".state") {
        let suffix = &lower[pos + ".state".len()..];
        let slot = if suffix.is_empty() {
            "slot0".to_owned()
        } else if suffix.chars().all(|c| c.is_ascii_digit()) {
            format!("slot{suffix}")
        } else {
            return None;
        };
        return Some((file_name[..pos].to_owned(), slot));
    }
    for ext in [".srm", ".sav"] {
        if let Some(stem) = lower.strip_suffix(ext) {
            // Opera: "<name>.0.srm" — drop the trailing disc/save index.
            let base = &file_name[..stem.len()];
            let base = base
                .rsplit_once('.')
                .filter(|(_, tail)| tail.chars().all(|c| c.is_ascii_digit()))
                .map(|(head, _)| head)
                .unwrap_or(base);
            return Some((base.to_owned(), "autosave".into()));
        }
    }
    None
}

/// Candidate platforms for a core stem, from the ES-DE core map.
fn platforms_for_core(map: &CoreMap, core: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (platform, default) in &map.default_core_by_romm_platform {
        if default == core && !out.contains(platform) {
            out.push(platform.clone());
        }
    }
    for platform in map.platforms_with_core(core) {
        if !out.contains(&platform) {
            out.push(platform);
        }
    }
    out
}

fn resolve(cache: &Cache, platforms: &[String], rom_base: &str) -> Result<Resolution> {
    let mut hits: Vec<(i64, String, String)> = Vec::new();
    for platform in platforms {
        for rom in cache.roms_for(platform)? {
            let stem = Path::new(&rom.fs_name)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(&rom.fs_name);
            if stem.eq_ignore_ascii_case(rom_base) {
                hits.push((rom.id, platform.clone(), rom.fs_name.clone()));
            }
        }
    }
    Ok(match hits.len() {
        0 => Resolution::Unmatched,
        1 => {
            let (rom_id, platform, fs_name) = hits.pop().unwrap();
            Resolution::Resolved { rom_id, platform, fs_name }
        }
        _ => Resolution::Ambiguous(hits),
    })
}

/// Walk `<root>/saves` and `<root>/states`.
pub fn scan(root: &Path, cache: &Cache, map: &CoreMap) -> Result<Vec<Candidate>> {
    let mut out = Vec::new();
    for (sub, kind) in [("saves", Kind::Save), ("states", Kind::State)] {
        let base = root.join(sub);
        if !base.is_dir() {
            continue;
        }
        collect(&base, &base, kind, cache, map, &mut out)?;
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    mark_canonical(&mut out, map);
    Ok(out)
}

/// Resolve `(rom_id, slot)` collisions between cores.
///
/// The same game saved under two cores maps to one server-side pairing — this
/// collection has Castlevania under both `melonDS` and `melonDS DS`. The
/// platform's default core wins, because that is what ES-DE launches and
/// therefore what future saves will come from. The loser is kept in the scan
/// output but flagged, never silently dropped.
fn mark_canonical(candidates: &mut [Candidate], map: &CoreMap) {
    use std::collections::HashMap;

    // (rom_id, slot, kind) -> index of the current best candidate.
    let mut best: HashMap<(i64, String, bool), usize> = HashMap::new();
    let mut losers: Vec<(usize, String)> = Vec::new();

    for i in 0..candidates.len() {
        let Resolution::Resolved { rom_id, platform, .. } = &candidates[i].resolution else {
            continue;
        };
        let key = (*rom_id, candidates[i].slot.clone(), candidates[i].kind == Kind::Save);
        let is_default = map.default_core(platform) == candidates[i].core.as_deref();

        match best.get(&key).copied() {
            None => {
                best.insert(key, i);
            }
            Some(prev) => {
                let prev_is_default = match &candidates[prev].resolution {
                    Resolution::Resolved { platform, .. } => {
                        map.default_core(platform) == candidates[prev].core.as_deref()
                    }
                    _ => false,
                };
                // Only displace the incumbent if it is not already the default.
                if is_default && !prev_is_default {
                    losers.push((prev, candidates[i].core.clone().unwrap_or_default()));
                    best.insert(key, i);
                } else {
                    losers.push((i, candidates[prev].core.clone().unwrap_or_default()));
                }
            }
        }
    }

    for (idx, winner) in losers {
        candidates[idx].canonical = false;
        candidates[idx].superseded_by = Some(winner);
    }
}

fn collect(
    base: &Path,
    dir: &Path,
    kind: Kind,
    cache: &Cache,
    map: &CoreMap,
    out: &mut Vec<Candidate>,
) -> Result<()> {
    for entry in std::fs::read_dir(dir)?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(base, &path, kind, cache, map, out)?;
            continue;
        }
        let file_name = entry.file_name().to_string_lossy().to_string();
        let rel = path.strip_prefix(base).unwrap_or(&path);
        if is_ignorable(rel, &file_name) {
            continue;
        }
        let Some((rom_base, slot)) = split_slot(&file_name) else {
            continue;
        };

        // First path component under saves/ or states/ is the core directory.
        let core_dir = rel
            .components()
            .next()
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .unwrap_or_default();
        let core = map.core_by_display_name(&core_dir, normalise);

        let resolution = match &core {
            None => Resolution::UnknownCore,
            Some(c) => {
                let platforms = platforms_for_core(map, c);
                if platforms.is_empty() {
                    Resolution::Unmatched
                } else {
                    resolve(cache, &platforms, &rom_base)?
                }
            }
        };

        out.push(Candidate {
            size: std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0),
            content_hash: savehash::compute(&path)?,
            path,
            kind,
            core_dir,
            core,
            rom_base,
            slot,
            resolution,
            canonical: true,
            superseded_by: None,
        });
    }
    Ok(())
}
