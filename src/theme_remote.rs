//! ES-DE theme downloader.
//!
//! Reads the official themes list and clones themes with `git`. Themes land in
//! our own directory (`<library>/themes/`) rather than `~/ES-DE/themes`, so
//! downloading one here never disturbs an existing ES-DE install — and it
//! stays inside the single deletable folder all our data lives in.
//!
//! Cloning is `--depth 1`: several themes carry hundreds of megabytes of
//! artwork and there is no reason to fetch their history.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

const THEMES_LIST_URL: &str =
    "https://gitlab.com/es-de/themes/themes-list/-/raw/master/themes.json";

#[derive(Debug, Deserialize)]
struct ThemesList {
    #[serde(default)]
    themes: Vec<RemoteTheme>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteTheme {
    pub name: String,
    /// Directory name the theme is cloned into; also its stable identifier.
    #[serde(default)]
    pub reponame: String,
    pub url: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub variants: Vec<String>,
    #[serde(default)]
    pub color_schemes: Vec<String>,
    #[serde(default)]
    pub screenshots: Vec<Screenshot>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Screenshot {
    #[serde(default)]
    pub image: String,
    #[serde(default)]
    pub caption: String,
}

/// Base for screenshot paths, which are relative to the themes-list repo.
const SCREENSHOT_BASE: &str =
    "https://gitlab.com/es-de/themes/themes-list/-/raw/master/";

impl RemoteTheme {
    /// Directory name to clone into, falling back to a slug of the name.
    pub fn dir_name(&self) -> String {
        if !self.reponame.is_empty() {
            return self.reponame.clone();
        }
        self.name
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
            .collect()
    }

    /// Absolute URL of the first screenshot, for preview thumbnails.
    ///
    /// Hosted on gitlab.com, not the RomM server — previewing a theme is
    /// inherently a network operation, since the next step clones it.
    pub fn screenshot_url(&self) -> Option<String> {
        let s = self.screenshots.first()?;
        (!s.image.is_empty()).then(|| format!("{SCREENSHOT_BASE}{}", s.image))
    }

    /// Loose match on name or reponame, for CLI lookup.
    pub fn matches(&self, needle: &str) -> bool {
        let n = needle.to_ascii_lowercase();
        self.name.to_ascii_lowercase().contains(&n)
            || self.reponame.to_ascii_lowercase().contains(&n)
    }
}

/// Fetch the official themes list with a throwaway client.
///
/// Themes are browsable with no RomM server configured, so this does not
/// depend on the API client existing.
pub async fn list_default() -> Result<Vec<RemoteTheme>> {
    let http = reqwest::Client::builder()
        .user_agent("romm-desktop/0.1")
        .build()
        .context("building HTTP client")?;
    list(&http).await
}

/// Fetch the official themes list.
pub async fn list(http: &reqwest::Client) -> Result<Vec<RemoteTheme>> {
    let resp = http
        .get(THEMES_LIST_URL)
        .send()
        .await
        .with_context(|| format!("GET {THEMES_LIST_URL}"))?;
    if !resp.status().is_success() {
        bail!("{THEMES_LIST_URL} -> {}", resp.status());
    }
    let parsed: ThemesList = resp.json().await.context("parsing themes.json")?;
    Ok(parsed.themes)
}

fn run_git(args: &[&str], cwd: Option<&Path>) -> Result<()> {
    let mut cmd = Command::new("git");
    cmd.args(args);
    if let Some(d) = cwd {
        cmd.current_dir(d);
    }
    let out = cmd.output().context("running git (is it installed?)")?;
    if !out.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

/// Clone a theme into `themes_dir`, or update it if already present.
///
/// Returns the installed path and whether it was newly cloned.
pub fn install(theme: &RemoteTheme, themes_dir: &Path) -> Result<(PathBuf, bool)> {
    std::fs::create_dir_all(themes_dir)
        .with_context(|| format!("creating {}", themes_dir.display()))?;
    let dest = themes_dir.join(theme.dir_name());

    if dest.join(".git").is_dir() {
        // Themes are read-only for us, so a hard reset is safe and avoids
        // merge conflicts if upstream force-pushed.
        run_git(&["fetch", "--depth", "1", "origin"], Some(&dest))?;
        run_git(&["reset", "--hard", "FETCH_HEAD"], Some(&dest))?;
        return Ok((dest, false));
    }
    if dest.exists() {
        bail!(
            "{} already exists but is not a git checkout — remove it first",
            dest.display()
        );
    }

    run_git(
        &["clone", "--depth", "1", &theme.url, &dest.to_string_lossy()],
        None,
    )?;
    Ok((dest, true))
}

/// Delete an installed theme.
pub fn remove(dir_name: &str, themes_dir: &Path) -> Result<()> {
    let dest = themes_dir.join(dir_name);
    if !dest.is_dir() {
        bail!("{} is not installed", dir_name);
    }
    std::fs::remove_dir_all(&dest).with_context(|| format!("removing {}", dest.display()))?;
    Ok(())
}

/// Recursive size on disk, for reporting what a theme costs.
pub use crate::util::dir_size as size_of;
