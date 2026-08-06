//! Save synchronisation: the glue between [`crate::saves`] and [`crate::api`].
//!
//! Both ends were already written and neither was ever called, so save sync has
//! never run. This module is the missing middle: scan the RetroArch save tree,
//! tell the server what we have, and carry out whatever it asks for.
//!
//! Deliberately explicit rather than automatic on launch. A save file is the
//! one thing here that cannot be re-downloaded if it goes wrong, so overwriting
//! one is a decision the user makes, not a side effect of pressing A.
//!
//! Conflicts are never resolved locally. When both sides changed since the last
//! sync the server says so and this reports it untouched — picking a winner
//! silently is how people lose an evening's progress.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::api::{Client, ClientSaveState};
use crate::cache::Cache;
use crate::coremap::CoreMap;
use crate::saves::{self, Candidate, Resolution};

/// This machine's identity with the server, persisted beside the cache.
///
/// The server issues the id; we only remember it. Without that the server sees
/// a new device on every run and cannot tell "this machine already has that
/// save" from "another machine is uploading a different one".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceIdentity {
    pub device_id: String,
    #[serde(default)]
    pub name: Option<String>,
}

const STATE_FILE: &str = "state.json";

impl DeviceIdentity {
    fn load(dir: &Path) -> Option<Self> {
        let raw = std::fs::read_to_string(dir.join(STATE_FILE)).ok()?;
        serde_json::from_str(&raw).ok()
    }

    fn save(&self, dir: &Path) -> Result<()> {
        let path = dir.join(STATE_FILE);
        let body = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, body).with_context(|| format!("writing {}", path.display()))
    }

    /// The stored identity, registering with the server the first time.
    pub async fn ensure(client: &Client, dir: &Path) -> Result<Self> {
        if let Some(existing) = Self::load(dir) {
            return Ok(existing);
        }
        let host = hostname();
        let device = client
            .register_device(&host, &host)
            .await
            .context("registering this device with the server")?;
        let identity = Self { device_id: device.id, name: device.name.or(Some(host)) };
        identity.save(dir)?;
        Ok(identity)
    }
}

/// Best-effort machine name, for the device list on the server.
fn hostname() -> String {
    for key in ["HOSTNAME", "COMPUTERNAME", "NAME"] {
        if let Some(v) = std::env::var_os(key)
            && !v.is_empty()
        {
            return v.to_string_lossy().into_owned();
        }
    }
    // gethostname without a dependency: the file exists on Linux, and the two
    // environment variables above cover macOS and Windows.
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_owned())
}

/// What a sync did, for reporting. Nothing here is a failure on its own —
/// conflicts and skips are normal and worth showing rather than burying.
#[derive(Debug, Default, Serialize)]
pub struct Summary {
    pub uploaded: usize,
    pub downloaded: usize,
    pub conflicts: usize,
    pub unchanged: usize,
    /// Local files the server was not told about, and why.
    pub skipped: usize,
    /// One line each, in the order they happened.
    pub notes: Vec<String>,
}

impl Summary {
    pub fn headline(&self) -> String {
        if self.uploaded + self.downloaded + self.conflicts == 0 {
            return format!("Saves already in sync ({} checked)", self.unchanged);
        }
        let mut parts = Vec::new();
        if self.uploaded > 0 {
            parts.push(format!("{} uploaded", self.uploaded));
        }
        if self.downloaded > 0 {
            parts.push(format!("{} downloaded", self.downloaded));
        }
        if self.conflicts > 0 {
            parts.push(format!("{} in conflict", self.conflicts));
        }
        parts.join(", ")
    }
}

/// Turn scanner output into what the server wants to hear about.
///
/// Only canonical, ROM-matched candidates are sent. An ambiguous or unmatched
/// save has no `rom_id` to pair on, and a non-canonical one would fight the
/// file that beat it for the same `(rom_id, slot)` on every run.
pub fn client_states(candidates: &[Candidate]) -> (Vec<ClientSaveState>, usize) {
    let mut out = Vec::new();
    let mut skipped = 0;

    for c in candidates {
        let Resolution::Resolved { rom_id, .. } = c.resolution else {
            skipped += 1;
            continue;
        };
        if !c.canonical {
            skipped += 1;
            continue;
        }
        out.push(ClientSaveState {
            rom_id,
            file_name: c
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            slot: Some(c.slot.clone()),
            emulator: c.core.clone().or_else(|| Some(c.core_dir.clone())),
            content_hash: c.content_hash.clone(),
            updated_at: modified_rfc3339(&c.path),
            file_size_bytes: c.size as i64,
        });
    }
    (out, skipped)
}

/// Where a save the server sent us should land.
///
/// Mirrors RetroArch's own layout, which is what the scanner reads back:
/// `<root>/states/<core>/…` for save states, `<root>/saves/<core>/…` for
/// battery saves. Without the core directory a download would land somewhere
/// the next scan cannot see.
pub fn download_path(root: &Path, file_name: &str, emulator: Option<&str>) -> PathBuf {
    let kind = if saves::is_state_name(file_name) { "states" } else { "saves" };
    let mut path = root.join(kind);
    if let Some(core) = emulator.filter(|e| !e.is_empty()) {
        path = path.join(core);
    }
    path.join(file_name)
}

/// Scan the save tree. Split from [`run`] so a caller holding a lock on the
/// cache can drop it before any awaiting starts — the SQLite connection is not
/// `Sync`, so a future holding one across an await cannot be spawned.
pub fn scan(cache: &Cache, map: &CoreMap, ra_root: &Path) -> Result<Vec<Candidate>> {
    saves::scan(ra_root, cache, map).context("scanning the save tree")
}

/// Negotiate and carry out the plan for an already-scanned tree.
pub async fn run(
    client: &Client,
    candidates: &[Candidate],
    ra_root: &Path,
    data_dir: &Path,
) -> Result<Summary> {
    let identity = DeviceIdentity::ensure(client, data_dir).await?;
    let mut summary = Summary::default();

    let (states, skipped) = client_states(candidates);
    summary.skipped = skipped;
    if skipped > 0 {
        summary
            .notes
            .push(format!("{skipped} local file(s) skipped: no matching ROM, or superseded"));
    }

    let plan = client
        .negotiate(&identity.device_id, &states)
        .await
        .context("asking the server what to sync")?;

    for op in &plan.operations {
        match op.action.as_str() {
            "download" => {
                let Some(save_id) = op.save_id else {
                    summary.notes.push("server asked for a download with no save id".to_owned());
                    continue;
                };
                let name = op.file_name.clone().unwrap_or_else(|| format!("save-{save_id}"));
                match download_one(client, ra_root, save_id, &name, op.emulator.as_deref(), &identity)
                    .await
                {
                    Ok(path) => {
                        summary.downloaded += 1;
                        summary.notes.push(format!("downloaded {}", path.display()));
                    }
                    Err(e) => summary.notes.push(format!("could not download {name}: {e}")),
                }
            }
            "upload" => {
                let name = op.file_name.clone().unwrap_or_default();
                let Some(c) = candidates.iter().find(|c| {
                    c.path.file_name().is_some_and(|n| n.to_string_lossy() == name)
                }) else {
                    summary.notes.push(format!("server asked to upload {name}, which is not here"));
                    continue;
                };
                match upload_one(client, c, op.rom_id, &identity, plan.session_id).await {
                    Ok(true) => {
                        summary.uploaded += 1;
                        summary.notes.push(format!("uploaded {name}"));
                    }
                    // A conflict is the server declining, not an error.
                    Ok(false) => {
                        summary.conflicts += 1;
                        summary.notes.push(format!("{name}: server copy moved on, left alone"));
                    }
                    Err(e) => summary.notes.push(format!("could not upload {name}: {e}")),
                }
            }
            "conflict" => {
                summary.conflicts += 1;
                let name = op.file_name.clone().unwrap_or_default();
                let why = op.reason.clone().unwrap_or_else(|| "both sides changed".to_owned());
                summary.notes.push(format!("{name}: {why} — nothing written"));
            }
            "no_op" => summary.unchanged += 1,
            other => summary.notes.push(format!("unknown action {other:?} from the server")),
        }
    }

    // Closing the session is what lets the server release its locks; a failure
    // here has not lost anything, so it is reported rather than propagated.
    if let Some(id) = plan.session_id
        && let Err(e) = client.complete_session(id).await
    {
        summary.notes.push(format!("could not close the sync session: {e}"));
    }

    Ok(summary)
}

async fn download_one(
    client: &Client,
    root: &Path,
    save_id: i64,
    file_name: &str,
    emulator: Option<&str>,
    identity: &DeviceIdentity,
) -> Result<PathBuf> {
    let bytes = client.save_content(save_id, &identity.device_id).await?;
    let path = download_path(root, file_name, emulator);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("creating {}", dir.display()))?;
    }
    std::fs::write(&path, bytes).with_context(|| format!("writing {}", path.display()))?;
    // Only after the bytes are safely on disk: telling the server first would
    // have it believe we hold a save we failed to write.
    client.confirm_download(save_id).await?;
    Ok(path)
}

/// Returns false when the server refused because its copy moved on.
async fn upload_one(
    client: &Client,
    candidate: &Candidate,
    rom_id: i64,
    identity: &DeviceIdentity,
    session_id: Option<i64>,
) -> Result<bool> {
    let bytes = std::fs::read(&candidate.path)
        .with_context(|| format!("reading {}", candidate.path.display()))?;
    let name = candidate
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    let result = client
        .upload_save(
            rom_id,
            &name,
            bytes,
            Some(&candidate.slot),
            candidate.core.as_deref().or(Some(&candidate.core_dir)),
            &identity.device_id,
            session_id,
        )
        .await?;
    Ok(result.is_ok())
}

/// A file's mtime as RFC 3339 UTC, which is the format the server compares on.
///
/// Done by hand rather than with a date crate: this is the only place the
/// project needs one, and the conversion is a well-known closed form.
fn modified_rfc3339(path: &Path) -> String {
    let secs = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    rfc3339(secs)
}

/// Seconds since the Unix epoch as `YYYY-MM-DDTHH:MM:SSZ`.
pub fn rfc3339(secs: i64) -> String {
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let (y, mo, d) = civil_from_days(days);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

/// Howard Hinnant's days-from-civil, inverted. Exact for any date this will
/// ever see, and shorter than taking on a date dependency for one call site.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The format the server compares timestamps in. A wrong date here does
    /// not error — it silently makes every local save look older or newer than
    /// it is, which decides who wins a sync.
    #[test]
    fn timestamps_are_rfc3339_utc() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(1_000_000_000), "2001-09-09T01:46:40Z");
        assert_eq!(rfc3339(1_700_000_000), "2023-11-14T22:13:20Z");
        // A leap day, which is where a hand-rolled conversion goes wrong.
        assert_eq!(rfc3339(1_709_164_800), "2024-02-29T00:00:00Z");
        assert_eq!(rfc3339(951_782_400), "2000-02-29T00:00:00Z");
    }

    /// Save states and battery saves live in different trees. Putting one in
    /// the other's directory means the next scan never finds it again.
    #[test]
    fn downloads_land_where_the_scanner_will_find_them() {
        let root = Path::new("/ra");
        assert_eq!(
            download_path(root, "Game.state", Some("snes9x")),
            Path::new("/ra/states/snes9x/Game.state")
        );
        assert_eq!(
            download_path(root, "Game.state3", Some("snes9x")),
            Path::new("/ra/states/snes9x/Game.state3")
        );
        assert_eq!(
            download_path(root, "Game.srm", Some("snes9x")),
            Path::new("/ra/saves/snes9x/Game.srm")
        );
    }

    /// RetroArch nests by core; a download without one would land at the top of
    /// the tree where the scanner does not look.
    #[test]
    fn a_missing_emulator_does_not_produce_a_stray_directory() {
        assert_eq!(
            download_path(Path::new("/ra"), "Game.srm", None),
            Path::new("/ra/saves/Game.srm")
        );
        assert_eq!(
            download_path(Path::new("/ra"), "Game.srm", Some("")),
            Path::new("/ra/saves/Game.srm"),
            "an empty core name must not create an unnamed directory"
        );
    }

    fn candidate(name: &str, resolution: Resolution, canonical: bool) -> Candidate {
        Candidate {
            path: PathBuf::from(format!("/ra/saves/snes9x/{name}")),
            kind: saves::Kind::Save,
            core_dir: "Snes9x".to_owned(),
            core: Some("snes9x".to_owned()),
            rom_base: "Game".to_owned(),
            slot: "srm".to_owned(),
            size: 8192,
            content_hash: "abc123".to_owned(),
            resolution,
            canonical,
            superseded_by: None,
        }
    }

    /// Only saves that pair with a ROM are offered. An unmatched one has no
    /// rom_id, and a superseded one would fight the file that beat it for the
    /// same (rom_id, slot) on every single sync.
    #[test]
    fn only_canonical_matched_saves_are_offered() {
        let resolved = || Resolution::Resolved {
            rom_id: 7,
            platform: "snes".to_owned(),
            fs_name: "Game.sfc".to_owned(),
        };
        let candidates = vec![
            candidate("Game.srm", resolved(), true),
            candidate("Loser.srm", resolved(), false),
            candidate("Orphan.srm", Resolution::Unmatched, true),
            candidate("Unknown.srm", Resolution::UnknownCore, true),
            candidate("Two.srm", Resolution::Ambiguous(vec![]), true),
        ];

        let (states, skipped) = client_states(&candidates);
        assert_eq!(states.len(), 1, "only the canonical, resolved one");
        assert_eq!(skipped, 4);
        assert_eq!(states[0].rom_id, 7);
        assert_eq!(states[0].file_name, "Game.srm");
        assert_eq!(states[0].content_hash, "abc123");
        assert_eq!(states[0].emulator.as_deref(), Some("snes9x"));
    }

    /// The scanner names the core directory even when it cannot resolve it to
    /// a libretro stem. Sending nothing would lose the pairing entirely.
    #[test]
    fn an_unresolved_core_falls_back_to_its_directory_name() {
        let mut c = candidate(
            "Game.srm",
            Resolution::Resolved {
                rom_id: 7,
                platform: "snes".to_owned(),
                fs_name: "Game.sfc".to_owned(),
            },
            true,
        );
        c.core = None;
        let (states, _) = client_states(&[c]);
        assert_eq!(states[0].emulator.as_deref(), Some("Snes9x"));
    }

    #[test]
    fn the_headline_says_nothing_happened_when_nothing_did() {
        let s = Summary { unchanged: 12, ..Default::default() };
        assert_eq!(s.headline(), "Saves already in sync (12 checked)");
        let s = Summary { uploaded: 2, conflicts: 1, ..Default::default() };
        assert_eq!(s.headline(), "2 uploaded, 1 in conflict");
    }
}
