//! `config.toml` — see `config.example.toml` for the documented template.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub server: Server,
    #[serde(default)]
    pub library: Library,
    #[serde(default)]
    pub retroarch: RetroArchCfg,
    #[serde(default)]
    pub saves: SavesCfg,
    #[serde(default)]
    pub theme: ThemeCfg,
    #[serde(default)]
    pub cores: CoresCfg,
    #[serde(default)]
    pub shaders: ShadersCfg,
}

#[derive(Debug, Default, Deserialize)]
pub struct Server {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct Library {
    #[serde(default = "default_local_root")]
    pub local_root: String,
}

impl Default for Library {
    fn default() -> Self {
        Self {
            local_root: default_local_root(),
        }
    }
}

fn default_local_root() -> String {
    "./library".to_owned()
}

#[derive(Debug, Deserialize)]
pub struct SavesCfg {
    /// Directory containing `saves/` and `states/` subdirectories. In a
    /// portable RetroArch install this is the portable root itself.
    #[serde(default = "default_saves_root")]
    pub root: String,
}

impl Default for SavesCfg {
    fn default() -> Self {
        Self { root: default_saves_root() }
    }
}

fn default_saves_root() -> String {
    "./Saves".to_owned()
}

#[derive(Debug, Default, Deserialize)]
pub struct CoresCfg {
    /// Platform slug -> libretro core stem, overriding the ES-DE default.
    ///
    /// Needed when a collection's ROMs do not match what the default core
    /// expects — arcade romsets in particular are version-locked, and a
    /// MAME 2003-Plus set will not run under current MAME.
    #[serde(default)]
    pub overrides: BTreeMap<String, String>,

    /// `"<platform>/<fs_name>"` -> core, for the one game that needs a
    /// different core from the rest of its platform.
    ///
    /// Keyed by path rather than ROM id: ids are reassigned when the server
    /// rescans, and a rebuilt library would silently point these at the wrong
    /// games. The path is also readable, so this table can be hand-edited.
    #[serde(default)]
    pub per_game: BTreeMap<String, String>,
}

/// Config key for a single game's core override.
pub fn game_key(platform: &str, fs_name: &str) -> String {
    format!("{platform}/{fs_name}")
}

#[derive(Debug, Deserialize)]
pub struct ShadersCfg {
    /// Master switch. Off means no shader is applied and RetroArch's own
    /// setting is left alone.
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Platform slug -> preset path under `shaders_slang/` (no extension), or
    /// `"none"` to force no shader for that platform.
    #[serde(default)]
    pub by_platform: BTreeMap<String, String>,
}

impl Default for ShadersCfg {
    fn default() -> Self {
        Self { enabled: true, by_platform: BTreeMap::new() }
    }
}

#[derive(Debug, Default, Deserialize)]
pub struct ThemeCfg {
    /// Extra directory to search for ES-DE themes, checked before the
    /// standard locations. Unset means probe ~/ES-DE/themes and the ES-DE.app
    /// bundle.
    pub root: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct RetroArchCfg {
    /// Single install. Kept for older configs; `installs` supersedes it.
    pub root: Option<String>,

    /// Ordered list of installs, tried top to bottom like a boot order. The
    /// first enabled entry that actually contains RetroArch wins, so a
    /// portable build can shadow a system one without deleting either.
    #[serde(default)]
    pub installs: Vec<RetroArchInstall>,

    /// Extra RetroArch settings appended at launch, on top of ours.
    ///
    /// Path to a file in RetroArch's own `key = "value"` format. Anything here
    /// wins, so button maps and video filters can be pinned without ever
    /// opening RetroArch's menu.
    pub user_config: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RetroArchInstall {
    /// Directory containing `RetroArch.app` (macOS) or `retroarch.exe`.
    pub path: String,
    /// Shown in Settings; defaults to the path when absent.
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

impl RetroArchCfg {
    /// Install paths to try, in order, honouring `enabled`.
    ///
    /// Falls back to the legacy single `root` so an existing config keeps
    /// working untouched.
    pub fn ordered_paths(&self) -> Vec<String> {
        if !self.installs.is_empty() {
            return self
                .installs
                .iter()
                .filter(|i| i.enabled)
                .map(|i| i.path.clone())
                .collect();
        }
        self.root.clone().into_iter().collect()
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        Self::load_from(Path::new("config.toml"))
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        if !path.is_file() {
            // Absent config is fine for commands that don't touch the server.
            return Ok(Self::default());
        }
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&raw).with_context(|| format!("parsing {}", path.display()))
    }

    pub fn local_roms_dir(&self) -> PathBuf {
        PathBuf::from(&self.library.local_root).join("roms")
    }

    pub fn media_dir(&self) -> PathBuf {
        PathBuf::from(&self.library.local_root).join("downloaded_media")
    }

    /// The user's own RetroArch settings file, appended at launch.
    pub fn user_retroarch_config(&self) -> PathBuf {
        match &self.retroarch.user_config {
            Some(p) => crate::util::expand_tilde(p),
            None => PathBuf::from(&self.library.local_root).join("retroarch-user.cfg"),
        }
    }

    /// Where downloaded ES-DE themes go. Inside the library folder so the
    /// "delete one folder to reclaim everything" property holds.
    pub fn themes_dir(&self) -> PathBuf {
        PathBuf::from(&self.library.local_root).join("themes")
    }
}


/// Set `key = "value"` inside `[table]` in a TOML file, creating the table if
/// needed.
///
/// A targeted text edit rather than parse-and-reserialise: the config carries
/// hand-written comments explaining non-obvious choices (which arcade core and
/// why, what the shader groups mean), and round-tripping through a serialiser
/// would delete all of them.
pub fn set_table_entry(path: &str, table: &str, key: &str, value: &str) -> Result<()> {
    write_entry(path, table, key, Some(value))
}

/// Remove `key` from `[table]` if present.
pub fn clear_table_entry(path: &str, table: &str, key: &str) -> Result<()> {
    write_entry(path, table, key, None)
}

/// TOML bare keys allow only letters, digits, `_` and `-`. Per-game keys are
/// file paths, so they have to be quoted and escaped.
fn toml_key(key: &str) -> String {
    if !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return key.to_owned();
    }
    format!("\"{}\"", key.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Does this line define `key`, in either its bare or quoted form?
fn defines_key(line: &str, key: &str) -> bool {
    let Some(lhs) = line.split('=').next() else {
        return false;
    };
    let lhs = lhs.trim();
    lhs == key || lhs == toml_key(key)
}

fn write_entry(path: &str, table: &str, key: &str, value: Option<&str>) -> Result<()> {
    let file = Path::new(path);
    let original = std::fs::read_to_string(file).unwrap_or_default();
    let header = format!("[{table}]");
    let entry = value.map(|v| {
        format!(
            "{} = \"{}\"",
            toml_key(key),
            v.replace('\\', "\\\\").replace('"', "\\\"")
        )
    });

    let mut lines: Vec<String> = original.lines().map(str::to_owned).collect();

    // Locate the table, and the key within it.
    let table_at = lines.iter().position(|l| l.trim() == header);
    let Some(start) = table_at else {
        // Nothing to remove from a table that does not exist.
        let Some(entry) = entry else { return Ok(()) };
        // No such table yet: append it.
        if !lines.is_empty() && !lines.last().is_some_and(|l| l.trim().is_empty()) {
            lines.push(String::new());
        }
        lines.push(header);
        lines.push(entry);
        std::fs::write(file, lines.join("\n") + "\n")
            .with_context(|| format!("writing {}", file.display()))?;
        return Ok(());
    };

    // The table ends at the next header line.
    let end = lines[start + 1..]
        .iter()
        .position(|l| l.trim_start().starts_with('['))
        .map(|i| start + 1 + i)
        .unwrap_or(lines.len());

    let existing = lines[start + 1..end]
        .iter()
        .position(|l| defines_key(l, key))
        .map(|i| start + 1 + i);

    match (existing, entry) {
        (Some(i), Some(entry)) => lines[i] = entry,
        (Some(i), None) => {
            lines.remove(i);
        }
        (None, Some(entry)) => lines.insert(end, entry),
        (None, None) => return Ok(()),
    }
    std::fs::write(file, lines.join("\n") + "\n")
        .with_context(|| format!("writing {}", file.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Per-game keys are file paths, which TOML cannot express as bare keys.
    /// Round-tripping through a real parser is the only check that matters.
    #[test]
    fn per_game_keys_round_trip() {
        let dir = std::env::temp_dir().join(format!("romm-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.toml");
        let p = path.to_str().unwrap();
        std::fs::write(&path, "[cores.overrides]\narcade = \"mame2003_plus\"\n").unwrap();

        // Names taken from the real library: spaces, dots, brackets, commas,
        // apostrophes, and a quote for good measure.
        let keys = [
            "arcade/blazstar.zip",
            "psx/Final Fantasy VII (USA) (Disc 1).chd",
            "snes/Blow'em Out! (USA) (Aftermarket) (Unl).zip",
            "nes/Say \"Hello\" [b1].nes",
            "gba/back\\slash.gba",
        ];
        for (i, k) in keys.iter().enumerate() {
            set_table_entry(p, "cores.per_game", k, &format!("core{i}")).unwrap();
        }

        let parsed: toml::Value = toml::from_str(&std::fs::read_to_string(&path).unwrap())
            .expect("must still be valid TOML");
        let tbl = parsed["cores"]["per_game"].as_table().unwrap();
        assert_eq!(tbl.len(), keys.len(), "one entry per key, no duplicates");
        for (i, k) in keys.iter().enumerate() {
            assert_eq!(tbl[*k].as_str(), Some(format!("core{i}").as_str()));
        }

        // Rewriting a key must replace, not append.
        set_table_entry(p, "cores.per_game", keys[0], "fbneo").unwrap();
        let parsed: toml::Value = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let tbl = parsed["cores"]["per_game"].as_table().unwrap();
        assert_eq!(tbl.len(), keys.len(), "rewrite must not duplicate");
        assert_eq!(tbl[keys[0]].as_str(), Some("fbneo"));

        // Clearing removes it.
        clear_table_entry(p, "cores.per_game", keys[0]).unwrap();
        let parsed: toml::Value = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(parsed["cores"]["per_game"].as_table().unwrap().len(), keys.len() - 1);

        // The hand-written table above must survive untouched.
        assert_eq!(parsed["cores"]["overrides"]["arcade"].as_str(), Some("mame2003_plus"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
