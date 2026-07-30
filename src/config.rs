//! `config.toml` — see `config.example.toml` for the documented template.

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

#[derive(Debug, Default, Deserialize)]
pub struct RetroArchCfg {
    /// Directory containing RetroArch.app. Unset means probe known locations.
    pub root: Option<String>,
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
}
