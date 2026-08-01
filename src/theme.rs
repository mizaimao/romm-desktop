//! ES-DE theme support — system logos for the platform grid.
//!
//! Reuses the themes ES-DE already has installed rather than fetching icons
//! from IGDB or TheGamesDB. RomM's `url_logo` points at those external CDNs,
//! not at your own server, so using it would mean a network round trip to a
//! third party for every console icon. Themes are local, offline, SVG, and
//! already match the frontend you use.
//!
//! Two directory conventions exist in the wild and both are handled:
//!
//! ```text
//! <theme>/<system>/images/logo.svg      slate-es-de, most community themes
//! <theme>/system/logos/<system>.svg     linear-es-de
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::coremap::CoreMap;

/// Where ES-DE keeps themes, in probe order.
const THEME_ROOTS: &[&str] = &[
    // User-installed themes take precedence over the bundled set.
    "~/ES-DE/themes",
    "~/.emulationstation/themes",
    "~/Data/Games/Emulators/ES-DE.app/Contents/Resources/themes",
    "/Applications/ES-DE.app/Contents/Resources/themes",
];

const ICON_EXTENSIONS: &[&str] = &["svg", "png", "webp", "jpg"];

fn expand_tilde(p: &str) -> PathBuf {
    match p.strip_prefix("~/") {
        Some(rest) => std::env::var_os("HOME")
            .map(|h| PathBuf::from(h).join(rest))
            .unwrap_or_else(|| PathBuf::from(p)),
        None => PathBuf::from(p),
    }
}

#[derive(Debug, Clone)]
pub struct Theme {
    pub name: String,
    pub path: PathBuf,
}

/// All themes found on this machine, deduplicated by name.
pub fn discover(extra_root: Option<&str>) -> Vec<Theme> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(e) = extra_root {
        roots.push(expand_tilde(e));
    }
    roots.extend(THEME_ROOTS.iter().map(|r| expand_tilde(r)));

    let mut out: Vec<Theme> = Vec::new();
    for root in roots {
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || out.iter().any(|t| t.name == name) {
                continue;
            }
            out.push(Theme { name, path });
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// RomM platform slug -> candidate ES-DE system names, best first.
///
/// The two disagree on several names (`ngc`/`gc`, `dc`/`dreamcast`,
/// `neogeoaes`/`neogeo`, `neo-geo-pocket`/`ngp`), and the mapping is already
/// recorded in the extracted core map, so derive it rather than restate it.
///
/// Returns every candidate rather than one, because ROM-hack systems claim the
/// same slug as their base system — `genh` and `msu-md` both map to
/// `megadrive` — and only the base system has theme art. Ordering puts an
/// exact slug match first, then the base system, then the hack variants.
pub fn slug_to_esde(map: &CoreMap) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (esde_name, system) in &map.systems {
        for platform in &system.romm_platforms {
            out.entry(platform.clone()).or_default().push(esde_name.clone());
        }
    }
    for (slug, names) in out.iter_mut() {
        // Exact match wins; otherwise shorter names are the base systems
        // ("megadrive" over "msu-md" is length-ambiguous, so exact-match and
        // the slug fallback below carry the real weight).
        names.sort_by_key(|n| (n != slug, n.len(), n.clone()));
        if !names.contains(slug) {
            names.push(slug.clone());
        }
    }
    out
}

fn first_existing(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates
        .iter()
        .find(|p| p.is_file())
        .and_then(|p| p.canonicalize().ok())
}

/// Locate a system logo within one theme.
pub fn logo_for(theme: &Theme, esde_system: &str) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    for ext in ICON_EXTENSIONS {
        // slate-es-de and most community themes
        candidates.push(theme.path.join(esde_system).join("images").join(format!("logo.{ext}")));
        // linear-es-de
        candidates.push(theme.path.join("system").join("logos").join(format!("{esde_system}.{ext}")));
    }
    first_existing(&candidates)
}

/// Resolve logos for every platform slug, using the first theme that has one.
///
/// Falling through theme by theme rather than requiring a single complete
/// theme means a sparse custom theme still contributes what it has.
pub fn logos(
    themes: &[Theme],
    map: &CoreMap,
    slugs: &[String],
) -> BTreeMap<String, PathBuf> {
    let names = slug_to_esde(map);
    let mut out = BTreeMap::new();
    'slug: for slug in slugs {
        let candidates = names
            .get(slug)
            .cloned()
            .unwrap_or_else(|| vec![slug.clone()]);
        for theme in themes {
            for esde in &candidates {
                if let Some(p) = logo_for(theme, esde) {
                    out.insert(slug.clone(), p);
                    continue 'slug;
                }
            }
        }
    }
    out
}

/// Copy resolved logos into the local media tree so the UI has a stable place
/// to read from and works even if ES-DE is later moved or uninstalled.
pub fn install(
    themes: &[Theme],
    map: &CoreMap,
    slugs: &[String],
    media_root: &Path,
) -> Result<usize> {
    let dir = media_root.join("_platforms");
    std::fs::create_dir_all(&dir)?;
    let mut n = 0;
    for (slug, src) in logos(themes, map, slugs) {
        let ext = src
            .extension()
            .map(|e| e.to_string_lossy().to_string())
            .unwrap_or_else(|| "svg".into());
        let dst = dir.join(format!("{slug}.{ext}"));
        if std::fs::copy(&src, &dst).is_ok() {
            n += 1;
        }
    }
    Ok(n)
}

/// Installed logo for a platform, if one has been copied in.
pub fn installed_logo(media_root: &Path, slug: &str) -> Option<PathBuf> {
    let dir = media_root.join("_platforms");
    for ext in ICON_EXTENSIONS {
        let p = dir.join(format!("{slug}.{ext}"));
        if p.is_file() {
            return p.canonicalize().ok();
        }
    }
    None
}
