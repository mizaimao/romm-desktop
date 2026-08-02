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
