//! Reading a local ES-DE library instead of a RomM server.
//!
//! The app was built around a remote server: sync metadata, download ROMs,
//! fetch artwork. An ES-DE install on a local disk already has all three, so
//! nothing needs downloading — only indexing.
//!
//! ES-DE's layout, and the one trap in it:
//!
//! ```text
//! <roms>/<system>/<game files>
//! <esde>/gamelists/<system>/gamelist.xml        names, genre, players, rating
//! <esde>/downloaded_media/<system>/<type>/<game stem>.<ext>
//! ```
//!
//! **`<system>` is ES-DE's name, not a RomM slug** — `dreamcast` vs `dc`,
//! `neogeo` vs `neogeoaes`, `megadrive` vs `megadrive`. The mapping already
//! exists in `data/esde-core-map.json`, which is what makes this cheap: the
//! same file that tells us which core runs a platform also tells us which
//! ES-DE system feeds it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::coremap::CoreMap;

/// One game found on disk.
#[derive(Debug, Clone)]
pub struct Game {
    /// RomM platform slug, so the rest of the app is unchanged.
    pub platform_slug: String,
    /// ES-DE system directory name, needed to find its media.
    pub system: String,
    pub name: String,
    pub fs_name: String,
    /// Absolute path — ES-DE libraries live wherever the user put them, so
    /// nothing may be assumed about a layout relative to the project.
    pub path: PathBuf,
    pub size_bytes: i64,
    pub summary: Option<String>,
    pub genres: Vec<String>,
    pub players: Option<String>,
    /// 0–100, converted from ES-DE's 0.0–1.0 so it matches RomM's scale.
    pub rating: Option<f64>,
    pub release_year: Option<i32>,
}

/// Where the pieces of an ES-DE install are.
#[derive(Debug, Clone)]
pub struct Layout {
    pub roms: PathBuf,
    pub gamelists: PathBuf,
    pub media: PathBuf,
}

impl Layout {
    /// Derive the layout from an ES-DE data directory, allowing an explicit
    /// ROMs directory since ES-DE keeps that separate and configurable.
    pub fn new(esde_root: &Path, roms: Option<&Path>) -> Self {
        Self {
            roms: roms.map(Path::to_path_buf).unwrap_or_else(|| esde_root.join("ROMs")),
            gamelists: esde_root.join("gamelists"),
            media: esde_root.join("downloaded_media"),
        }
    }
}

/// Metadata for one game out of a gamelist.
#[derive(Default)]
struct Entry {
    name: Option<String>,
    desc: Option<String>,
    genre: Option<String>,
    players: Option<String>,
    rating: Option<f64>,
    year: Option<i32>,
}

/// Parse `gamelist.xml` into `file stem -> metadata`.
///
/// Hand-rolled rather than pulled in as an XML dependency: the format is flat,
/// and the only awkward part is that `<path>` is relative and usually prefixed
/// `./`, so it is reduced to a file name for matching.
fn parse_gamelist(path: &Path) -> BTreeMap<String, Entry> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return BTreeMap::new();
    };
    let mut out = BTreeMap::new();
    for chunk in text.split("<game>").skip(1) {
        let body = chunk.split("</game>").next().unwrap_or("");
        let field = |tag: &str| -> Option<String> {
            let open = format!("<{tag}>");
            let close = format!("</{tag}>");
            let s = body.find(&open)? + open.len();
            let e = body[s..].find(&close)? + s;
            let v = unescape(body[s..e].trim());
            (!v.is_empty()).then_some(v)
        };
        let Some(p) = field("path") else { continue };
        let file = p.rsplit(['/', '\\']).next().unwrap_or(&p).to_owned();
        let key = file.rsplit_once('.').map_or(file.clone(), |(s, _)| s.to_owned());
        out.insert(
            key,
            Entry {
                name: field("name"),
                desc: field("desc"),
                genre: field("genre"),
                players: field("players"),
                // ES-DE stores 0.0–1.0; the rest of the app uses RomM's 0–100.
                rating: field("rating").and_then(|r| r.parse::<f64>().ok()).map(|r| r * 100.0),
                // "19940101T000000"
                year: field("releasedate")
                    .and_then(|d| d.get(..4).and_then(|y| y.parse().ok())),
            },
        );
    }
    out
}

fn unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

/// Files ES-DE keeps beside games that are not games.
fn is_game_file(p: &Path) -> bool {
    let Some(name) = p.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if name.starts_with('.') {
        return false;
    }
    !matches!(
        p.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref(),
        Some("xml" | "txt" | "srm" | "state" | "cfg" | "dat" | "db")
    )
}

/// Scan an ES-DE library into games, keyed to RomM platform slugs.
///
/// Systems the core map does not know are skipped rather than guessed at — a
/// wrong slug would put games under a platform whose core cannot run them.
pub fn scan(layout: &Layout, map: &CoreMap) -> Result<Vec<Game>> {
    if !layout.roms.is_dir() {
        bail!("no ROMs directory at {}", layout.roms.display());
    }

    // ES-DE system name -> RomM slug, from the map we already ship.
    let mut sys_to_slug: BTreeMap<&str, &str> = BTreeMap::new();
    for (system, def) in &map.systems {
        if let Some(slug) = def.romm_platforms.first() {
            sys_to_slug.insert(system.as_str(), slug.as_str());
        }
    }

    let mut out = Vec::new();
    for entry in std::fs::read_dir(&layout.roms)
        .with_context(|| format!("reading {}", layout.roms.display()))?
        .flatten()
    {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let Some(system) = dir.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(slug) = sys_to_slug.get(system) else {
            continue;
        };

        let meta = parse_gamelist(&layout.gamelists.join(system).join("gamelist.xml"));

        for f in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = f.path();
            // A directory here is a multi-disc game, which ES-DE treats as one
            // entry, same as RomM's folder ROMs.
            let is_dir = path.is_dir();
            if !is_dir && !is_game_file(&path) {
                continue;
            }
            let Some(fs_name) = path.file_name().and_then(|n| n.to_str()).map(str::to_owned) else {
                continue;
            };
            let stem = fs_name.rsplit_once('.').map_or(fs_name.clone(), |(s, _)| s.to_owned());
            let e = meta.get(&stem);
            let size = if is_dir {
                dir_size(&path)
            } else {
                std::fs::metadata(&path).map(|m| m.len() as i64).unwrap_or(0)
            };
            out.push(Game {
                platform_slug: (*slug).to_owned(),
                system: system.to_owned(),
                name: e.and_then(|e| e.name.clone()).unwrap_or_else(|| stem.clone()),
                fs_name,
                path,
                size_bytes: size,
                summary: e.and_then(|e| e.desc.clone()),
                genres: e
                    .and_then(|e| e.genre.clone())
                    .map(|g| g.split([',', '/']).map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()).collect())
                    .unwrap_or_default(),
                players: e.and_then(|e| e.players.clone()),
                rating: e.and_then(|e| e.rating),
                release_year: e.and_then(|e| e.year),
            });
        }
    }
    out.sort_by(|a, b| (&a.platform_slug, &a.name).cmp(&(&b.platform_slug, &b.name)));
    Ok(out)
}

fn dir_size(p: &Path) -> i64 {
    let Ok(rd) = std::fs::read_dir(p) else { return 0 };
    rd.flatten()
        .map(|e| {
            let q = e.path();
            if q.is_dir() { dir_size(&q) } else { std::fs::metadata(&q).map(|m| m.len() as i64).unwrap_or(0) }
        })
        .sum()
}

/// Find one piece of artwork for a game.
///
/// ES-DE names media after the game's file stem, so this needs the stem and the
/// *ES-DE system* name — not the RomM slug, which is why `Game` carries both.
pub fn media_path(layout: &Layout, system: &str, stem: &str, kind: &str) -> Option<PathBuf> {
    const EXTS: &[&str] = &["png", "jpg", "jpeg", "webp", "mp4", "webm", "mkv", "pdf"];
    let dir = layout.media.join(system).join(kind);
    EXTS.iter()
        .map(|e| dir.join(format!("{stem}.{e}")))
        .find(|p| p.is_file())
}
