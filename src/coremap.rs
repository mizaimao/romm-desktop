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
    /// What to call it on screen — "Sony PlayStation", not "psx".
    ///
    /// The table has carried this all along and nothing read it. A library
    /// scanned from a card knows only the directory name, so every console on
    /// the handheld was labelled `psx`, `gbc`, `dc`.
    #[serde(default)]
    pub fullname: Option<String>,
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

/// The shipped core map, compiled into the binary.
///
/// A file next to the executable is not something a downloaded build can rely
/// on: the resource-copying code only ever knew the macOS `.app` layout, so a
/// loose Windows exe found nothing, failed to load the map, and — with no
/// console attached in a release build — exited without a word. 110 KB in the
/// binary buys a program that always starts.
pub const EMBEDDED: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/esde-core-map.json"));

impl CoreMap {
    /// The compiled-in map. Infallible by construction: it is validated at
    /// build time by the test below, so a malformed file fails CI, not a user.
    /// What a console directory is called on screen.
    ///
    /// Looked up by the *system* name — the directory on the card — and then by
    /// any RomM slug that maps to it, because a scan files games under the slug
    /// and the table is keyed by the system.
    pub fn display_name(&self, slug: &str) -> Option<&str> {
        if let Some(name) = self.systems.get(slug).and_then(|s| s.fullname.as_deref()) {
            return Some(name);
        }
        self.systems
            .values()
            .find(|sys| sys.romm_platforms.iter().any(|p| p == slug))
            .and_then(|sys| sys.fullname.as_deref())
    }

    pub fn embedded() -> Self {
        serde_json::from_str(EMBEDDED).expect("the embedded core map is valid JSON")
    }

    /// Load from disk, falling back to the embedded copy.
    ///
    /// Disk wins so a user can edit the mapping without a rebuild; the
    /// fallback means a fresh install works before anything has been written.
    pub fn load_or_embedded(path: &Path) -> Self {
        Self::load(path).unwrap_or_else(|_| Self::embedded())
    }

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
    /// match on a normalized form of both the label and the stem itself.
    pub fn core_by_display_name(
        &self,
        display: &str,
        normalize: fn(&str) -> String,
    ) -> Option<String> {
        let want = normalize(display);
        if want.is_empty() {
            return None;
        }
        let mut fallback = None;
        for system in self.systems.values() {
            for emu in &system.emulators {
                let Some(core) = emu.core.as_deref() else {
                    continue;
                };
                if normalize(core) == want {
                    return Some(core.to_owned());
                }
                if normalize(&emu.label) == want {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn map() -> CoreMap {
        serde_json::from_str(
            r#"{
              "default_core_by_romm_platform": {"snes": "snes9x"},
              "systems": {"snes": {
                "romm_platforms": ["snes"],
                "emulators": [
                  {"label": "Snes9x", "kind": "libretro", "core": "snes9x"},
                  {"label": "bsnes",  "kind": "libretro", "core": "bsnes"},
                  {"label": "Standalone", "kind": "standalone"}
                ]}}
            }"#,
        )
        .expect("test fixture matches the CoreMap schema")
    }

    /// A per-game pin beats the platform override, which beats the ES-DE
    /// default. This ordering is why arcade works at all: one platform, many
    /// romsets, no single core that runs them.
    /// The compiled-in map has to parse, since `embedded()` unwraps it. This
    /// is what makes that unwrap safe: a bad `data/esde-core-map.json` fails
    /// here rather than at a user's first launch.
    #[test]
    fn the_embedded_core_map_parses() {
        let m = CoreMap::embedded();
        assert!(!m.systems.is_empty(), "the embedded map has systems");
        assert!(
            m.default_core("snes").is_some(),
            "and a default for a platform that certainly has one"
        );
    }

    #[test]
    fn per_game_beats_override_beats_default() {
        let m = map();
        let none = BTreeMap::new();
        let over = BTreeMap::from([("snes".to_owned(), "bsnes".to_owned())]);
        let pinned = BTreeMap::from([(
            crate::config::game_key("snes", "Game.sfc"),
            "mesen".to_owned(),
        )]);

        assert_eq!(
            resolve_core_for(&m, &over, &pinned, "snes", Some("Game.sfc"), |_| true).as_deref(),
            Some("mesen"),
            "a per-game pin must win over everything"
        );
        assert_eq!(
            resolve_core_for(&m, &over, &none, "snes", Some("Game.sfc"), |_| true).as_deref(),
            Some("bsnes"),
            "with no pin, the platform override wins"
        );
        assert_eq!(
            resolve_core_for(&m, &none, &none, "snes", None, |_| true).as_deref(),
            Some("snes9x"),
            "with neither, the ES-DE default is used"
        );
    }

    /// A pin only applies to the game it names — a sibling on the same platform
    /// still resolves normally.
    #[test]
    fn a_pin_does_not_leak_to_other_games() {
        let m = map();
        let pinned = BTreeMap::from([(
            crate::config::game_key("snes", "Game.sfc"),
            "mesen".to_owned(),
        )]);
        assert_eq!(
            resolve_core_for(&m, &BTreeMap::new(), &pinned, "snes", Some("Other.sfc"), |_| true)
                .as_deref(),
            Some("snes9x")
        );
    }

    /// An override is honoured even when that core is absent, so the resulting
    /// error names the core the user asked for instead of quietly substituting.
    #[test]
    fn override_survives_a_missing_core() {
        let m = map();
        let over = BTreeMap::from([("snes".to_owned(), "bsnes".to_owned())]);
        assert_eq!(
            resolve_core_for(&m, &over, &BTreeMap::new(), "snes", None, |_| false).as_deref(),
            Some("bsnes")
        );
    }

    /// An uninstalled default falls through to an installed alternative rather
    /// than failing the launch.
    #[test]
    fn uninstalled_default_falls_back_to_an_alternative() {
        let m = map();
        let none = BTreeMap::new();
        assert_eq!(
            resolve_core_for(&m, &none, &none, "snes", None, |c| c == "bsnes").as_deref(),
            Some("bsnes")
        );
        assert_eq!(
            resolve_core_for(&m, &none, &none, "snes", None, |_| false),
            None,
            "with nothing installed there is nothing to resolve to"
        );
    }

    #[test]
    fn unknown_platform_resolves_to_nothing() {
        let m = map();
        let none = BTreeMap::new();
        assert_eq!(resolve_core_for(&m, &none, &none, "dreamcast", None, |_| true), None);
    }

    /// Standalone emulators are not cores and must never be handed to RetroArch.
    #[test]
    fn standalone_emulators_are_not_offered_as_cores() {
        assert_eq!(map().alternatives("snes"), vec!["snes9x", "bsnes"]);
    }
}
