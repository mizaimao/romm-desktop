//! ES-DE theme support — system logos for the platform grid.
//!
//! Reuses the themes ES-DE already has installed rather than fetching icons
//! from IGDB or TheGamesDB. RomM's `url_logo` points at those external CDNs,
//! not at your own server, so using it would mean a network round trip to a
//! third party for every console icon. Themes are local, offline, SVG, and
//! already match the frontend you use.
//!
//! Themes disagree on where per-system art lives. Three conventions are
//! checked directly, and anything else is found by a depth-limited sweep for
//! a directory whose name contains "logo":
//!
//! ```text
//! <theme>/<system>/images/<style>.svg     slate-es-de, most community themes
//! <theme>/system/logos/<system>.svg       linear-es-de
//! <theme>/_inc/system-logo/<system>.svg   canvas-es-de
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::util::expand_tilde;

use crate::coremap::CoreMap;

/// Where ES-DE keeps themes, in probe order.
const THEME_ROOTS: &[&str] = &[
    // User-installed themes take precedence over the bundled set.
    "~/ES-DE/themes",
    "~/.emulationstation/themes",
    "~/Data/Games/Emulators/ES-DE.app/Contents/Resources/themes",
    "/Applications/ES-DE.app/Contents/Resources/themes",
];

// svg first (crisp at any size), then the raster formats themes use.
const ICON_EXTENSIONS: &[&str] = &["svg", "webp", "png", "jpg"];

/// Which piece of per-system art to use for the platform grid.
///
/// ES-DE themes ship several. In the bundled slate theme: 201 `logo.svg`
/// (wordmarks), 89 `consolegame.svg` (console with a game), 69
/// `controller.svg`. Coverage varies by theme and system, so callers should be
/// ready to fall back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconStyle {
    /// Stylised wordmark. Best coverage.
    Logo,
    /// The console itself, usually with a cartridge or disc.
    ConsoleGame,
    /// The system's controller.
    Controller,
    /// Rendered hardware art (modern-es-de `art/`, linear-es-de `systemart/`).
    SystemArt,
    /// The older hardware renders modern-es-de keeps alongside the current set.
    SystemArtLegacy,
}

impl IconStyle {
    pub const ALL: [IconStyle; 5] = [
        Self::Logo,
        Self::ConsoleGame,
        Self::Controller,
        Self::SystemArt,
        Self::SystemArtLegacy,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Self::Logo => "logo",
            Self::ConsoleGame => "consolegame",
            Self::Controller => "controller",
            Self::SystemArt => "systemart",
            Self::SystemArtLegacy => "systemart_legacy",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Logo => "Logos",
            Self::ConsoleGame => "Consoles",
            Self::Controller => "Controllers",
            Self::SystemArt => "Hardware",
            Self::SystemArtLegacy => "Hardware (classic)",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.key() == s.to_ascii_lowercase())
    }
}

#[derive(Debug, Clone)]
pub struct Theme {
    pub name: String,
    pub path: PathBuf,
}

/// All themes found on this machine, deduplicated by name.
pub fn discover(extra_root: Option<&str>) -> Vec<Theme> {
    discover_with(extra_root, None)
}

/// As [`discover`], but also searching a downloaded-themes directory first.
pub fn discover_with(extra_root: Option<&str>, downloaded: Option<&Path>) -> Vec<Theme> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(d) = downloaded {
        roots.push(d.to_path_buf());
    }
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
///
/// Themes do not agree on where logos live. Three conventions are common
/// enough to check directly; anything else is found by the generic sweep in
/// [`logo_by_sweep`].
pub fn logo_for(theme: &Theme, esde_system: &str) -> Option<PathBuf> {
    art_for(theme, esde_system, IconStyle::Logo)
}

/// Locate one style of per-system art within a theme.
pub fn art_for(theme: &Theme, esde_system: &str, style: IconStyle) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    for ext in ICON_EXTENSIONS {
        // slate-es-de and many community themes keep every style side by side.
        candidates.push(
            theme.path.join(esde_system).join("images").join(format!("{}.{ext}", style.key())),
        );
    }
    // Hardware renders live in their own top-level directories rather than
    // per-system folders: modern-es-de uses art/ and art_legacy/, linear-es-de
    // uses system/systemart/.
    match style {
        IconStyle::SystemArt => {
            for ext in ICON_EXTENSIONS {
                candidates.push(theme.path.join("art").join(format!("{esde_system}.{ext}")));
                candidates
                    .push(theme.path.join("system").join("systemart").join(format!("{esde_system}.{ext}")));
            }
        }
        IconStyle::SystemArtLegacy => {
            for ext in ICON_EXTENSIONS {
                candidates.push(theme.path.join("art_legacy").join(format!("{esde_system}.{ext}")));
            }
        }
        _ => {}
    }
    if style == IconStyle::Logo {
        for ext in ICON_EXTENSIONS {
            // linear-es-de
            candidates.push(theme.path.join("system").join("logos").join(format!("{esde_system}.{ext}")));
            // canvas-es-de and relatives
            candidates.push(theme.path.join("_inc").join("system-logo").join(format!("{esde_system}.{ext}")));
        }
    }
    let found = first_existing(&candidates);
    match (found, style) {
        (Some(p), _) => Some(p),
        // Only the sweep knows about unusual layouts, and it only knows logos.
        (None, IconStyle::Logo) => logo_by_sweep(theme, esde_system),
        (None, _) => None,
    }
}

/// Fallback: look for `<system>.<ext>` inside any directory whose name mentions
/// "logo".
///
/// Depth-limited because theme repositories run to hundreds of megabytes and a
/// full walk per system would be slow; logo directories are always shallow.
fn logo_by_sweep(theme: &Theme, esde_system: &str) -> Option<PathBuf> {
    fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
        if depth == 0 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if name.starts_with('.') {
                continue;
            }
            if name.contains("logo") {
                out.push(path.clone());
            }
            walk(&path, depth - 1, out);
        }
    }

    let mut dirs = Vec::new();
    walk(&theme.path, 3, &mut dirs);
    for dir in dirs {
        for ext in ICON_EXTENSIONS {
            let p = dir.join(format!("{esde_system}.{ext}"));
            if p.is_file() {
                return p.canonicalize().ok();
            }
        }
    }
    None
}

/// Resolve logos for every platform slug, using the first theme that has one.
///
/// Falling through theme by theme rather than requiring a single complete
/// theme means a sparse custom theme still contributes what it has.
pub fn logos(themes: &[Theme], map: &CoreMap, slugs: &[String]) -> BTreeMap<String, PathBuf> {
    art(themes, map, slugs, IconStyle::Logo)
}

/// As [`logos`], for a specific art style.
pub fn art(
    themes: &[Theme],
    map: &CoreMap,
    slugs: &[String],
    style: IconStyle,
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
                if let Some(p) = art_for(theme, esde, style) {
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
    let mut total = 0;
    for style in IconStyle::ALL {
        total += install_style(themes, map, slugs, media_root, style)?;
    }
    Ok(total)
}

/// Install one art style into `_platforms/<style>/`.
///
/// Styles are kept side by side so switching in the UI needs no re-download.
pub fn install_style(
    themes: &[Theme],
    map: &CoreMap,
    slugs: &[String],
    media_root: &Path,
    style: IconStyle,
) -> Result<usize> {
    let dir = media_root.join("_platforms").join(style.key());
    std::fs::create_dir_all(&dir)?;
    let mut n = 0;
    for (slug, src) in art(themes, map, slugs, style) {
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

/// Installed art for a platform in the requested style, falling back to
/// `Logo` when that style has no art for this system.
pub fn installed_logo(media_root: &Path, slug: &str, style: IconStyle) -> Option<PathBuf> {
    let base = media_root.join("_platforms");
    let mut styles = vec![style];
    if style != IconStyle::Logo {
        styles.push(IconStyle::Logo);
    }
    for s in styles {
        for ext in ICON_EXTENSIONS {
            let p = base.join(s.key()).join(format!("{slug}.{ext}"));
            if p.is_file() {
                return p.canonicalize().ok();
            }
        }
    }
    None
}

/// How many platforms have art installed for each style.
pub fn installed_counts(media_root: &Path, slugs: &[String]) -> Vec<(IconStyle, usize)> {
    IconStyle::ALL
        .into_iter()
        .map(|style| {
            let base = media_root.join("_platforms").join(style.key());
            let n = slugs
                .iter()
                .filter(|slug| {
                    ICON_EXTENSIONS
                        .iter()
                        .any(|ext| base.join(format!("{slug}.{ext}")).is_file())
                })
                .count();
            (style, n)
        })
        .collect()
}
