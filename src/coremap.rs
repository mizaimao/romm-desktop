//! Reads `data/esde-core-map.json` — the platform → libretro core mapping
//! extracted from ES-DE (see `tools/extract_esde_cores.py`).

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct CoreMap {
    /// RomM platform slug -> default core stem, e.g. `"snes" -> "snes9x"`.
    pub default_core_by_romm_platform: BTreeMap<String, String>,
    pub systems: BTreeMap<String, System>,
}

#[derive(Debug, Deserialize)]
pub struct System {
    pub romm_platforms: Vec<String>,
    pub emulators: Vec<Emulator>,
}

#[derive(Debug, Deserialize)]
pub struct Emulator {
    pub label: String,
    pub kind: String,
    /// Present only when `kind == "libretro"`.
    #[serde(default)]
    pub core: Option<String>,
}

/// Pick the core for a platform: an explicit config override, else the ES-DE
/// default, else any installed alternative.
///
/// `has_core` lets callers supply their own installed-check without this
/// module depending on the RetroArch locator.
pub fn resolve_core(
    map: &CoreMap,
    overrides: &std::collections::BTreeMap<String, String>,
    platform: &str,
    has_core: impl Fn(&str) -> bool,
) -> Option<String> {
    resolve_core_for(map, overrides, &Default::default(), platform, None, has_core)
}

/// As [`resolve_core`], but a per-game override wins over the platform one.
///
/// Arcade is why this exists: no single core runs a mixed romset, so the
/// platform default is a best guess and individual games need to escape it.
pub fn resolve_core_for(
    map: &CoreMap,
    overrides: &std::collections::BTreeMap<String, String>,
    per_game: &std::collections::BTreeMap<String, String>,
    platform: &str,
    fs_name: Option<&str>,
    has_core: impl Fn(&str) -> bool,
) -> Option<String> {
    if let Some(fs_name) = fs_name
        && let Some(core) = per_game.get(&crate::config::game_key(platform, fs_name))
    {
        return Some(core.clone());
    }
    // An override is a deliberate choice; honour it even if not installed, so
    // the failure names the core the user asked for.
    if let Some(core) = overrides.get(platform) {
        return Some(core.clone());
    }
    if let Some(default) = map.default_core(platform)
        && has_core(default)
    {
        return Some(default.to_owned());
    }
    map.alternatives(platform)
        .into_iter()
        .find(|c| has_core(c))
        .map(str::to_owned)
}

impl CoreMap {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("reading core map at {}", path.display()))?;
        serde_json::from_str(&raw).with_context(|| format!("parsing {}", path.display()))
    }

    /// Default core stem for a RomM platform slug.
    pub fn default_core(&self, platform: &str) -> Option<&str> {
        self.default_core_by_romm_platform
            .get(platform)
            .map(String::as_str)
    }

    /// Every libretro core ES-DE offers for a platform, in preference order.
    /// Used for "launch with a different core" later.
    pub fn alternatives(&self, platform: &str) -> Vec<&str> {
        let mut out = Vec::new();
        for system in self.systems.values() {
            if !system.romm_platforms.iter().any(|p| p == platform) {
                continue;
            }
            for emu in &system.emulators {
                if emu.kind == "libretro"
                    && let Some(core) = emu.core.as_deref()
                    && !out.contains(&core)
                {
                    out.push(core);
                }
            }
        }
        out
    }

    /// Every RomM platform that lists `core` among its emulators.
    pub fn platforms_with_core(&self, core: &str) -> Vec<String> {
        let mut out = Vec::new();
        for system in self.systems.values() {
            let has = system
                .emulators
                .iter()
                .any(|e| e.core.as_deref() == Some(core));
            if has {
                for p in &system.romm_platforms {
                    if !out.contains(p) {
                        out.push(p.clone());
                    }
                }
            }
        }
        out
    }

    /// Map an emulator's *display name* to a core stem.
    ///
    /// RetroArch names its save subdirectories after the core's display name,
    /// which differs from ES-DE's labels (`"MAME"` vs `"MAME - Current"`), so
    /// match on a normalised form of both the label and the stem itself.
    pub fn core_by_display_name(
        &self,
        display: &str,
        normalise: fn(&str) -> String,
    ) -> Option<String> {
        let want = normalise(display);
        if want.is_empty() {
            return None;
        }
        let mut fallback = None;
        for system in self.systems.values() {
            for emu in &system.emulators {
                let Some(core) = emu.core.as_deref() else {
                    continue;
                };
                if normalise(core) == want {
                    return Some(core.to_owned());
                }
                if normalise(&emu.label) == want {
                    fallback.get_or_insert_with(|| core.to_owned());
                }
            }
        }
        fallback
    }

    /// Human label for a core, for display.
    pub fn label_for(&self, core: &str) -> Option<&str> {
        self.systems
            .values()
            .flat_map(|s| &s.emulators)
            .find(|e| e.core.as_deref() == Some(core))
            .map(|e| e.label.as_str())
    }
}
