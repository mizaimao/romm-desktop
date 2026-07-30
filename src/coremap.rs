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

    /// Human label for a core, for display.
    pub fn label_for(&self, core: &str) -> Option<&str> {
        self.systems
            .values()
            .flat_map(|s| &s.emulators)
            .find(|e| e.core.as_deref() == Some(core))
            .map(|e| e.label.as_str())
    }
}
