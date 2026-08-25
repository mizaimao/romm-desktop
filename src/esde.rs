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

/// ES-DE system directories the core map does not cover.
///
/// The map is built from ES-DE's Android system list, and a real install does
/// not match it exactly: `genesis` is the same console the map calls
/// `megadrive` — 942 games on this card, and the single biggest thing a
/// missing alias would drop. The rest are systems RomM has platforms for but
/// the Android export never listed.
const SYSTEM_ALIASES: &[(&str, &str)] = &[
    ("genesis", "megadrive"),
    ("megadrive", "megadrive"),
    ("gameandwatch", "g-and-w"),
    ("n3ds", "new-nintendo-3ds"),
    ("pico-8", "pico8"),
    ("easyrpg", "easyrpg"),
    ("ps2", "ps2"),
    ("ps3", "ps3"),
    ("switch", "switch"),
    ("wii", "wii"),
    ("wiiu", "wiiu"),
    ("saturn", "saturn"),
    ("xbox360", "xbox360"),
    ("naomi", "naomi"),
];

/// Directories inside the ROMs folder that are not systems.
///
/// A BIOS folder scanned as a system would invent 176 phantom "games" and,
/// worse, hand them to a core that cannot run them.
const NOT_SYSTEMS: &[&str] = &["0_BIOS", "bios", "ports", "SourcePorts", "Ports"];

/// One game found on disk.
#[derive(Debug, Clone, Default)]
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
pub fn scan(layout: &Layout, map: &CoreMap) -> Result<(Vec<Game>, Vec<String>)> {
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
    // Aliases win: they describe the directory names a real install uses.
    for (system, slug) in SYSTEM_ALIASES {
        sys_to_slug.insert(system, slug);
    }
    // The device's own spellings win over both. Batocera names its directories
    // itself, and the shipped core map was built from an ES-DE *Android*
    // export, so on KNULLI `fbneo` is the whole arcade library under a name
    // nothing else here knows.
    for (system, slug) in crate::platform::current().system_aliases() {
        sys_to_slug.insert(system, slug);
    }
    let ignored = crate::platform::current().ignored_systems();

    let mut out = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
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
        // `NOT_SYSTEMS` is what is never a system anywhere; `ignored` is what
        // this device has and does not want. Both are silent — unlike an
        // unknown directory, which is reported so a missing alias can be found.
        if NOT_SYSTEMS.contains(&system) || ignored.contains(&system) || system.starts_with('.')
        {
            continue;
        }
        let Some(slug) = sys_to_slug.get(system) else {
            skipped.push(system.to_owned());
            continue;
        };

        let meta = parse_gamelist(&layout.gamelists.join(system).join("gamelist.xml"));

        for f in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = f.path();
            // A directory here is a multi-disc game, which ES-DE treats as one
            // entry, same as RomM's folder ROMs.
            // A leading dot means hidden, and the device means it.
            //
            // Batocera's own front end skips these, and multi-disc games are
            // filed exactly this way: the discs go in `.Final Fantasy VII
            // (USA)/` and the `.m3u` beside it is the thing to launch. Scanning
            // the folder as well listed every one of those games twice — once
            // properly and once as `.Final Fantasy VII (USA)`, with a dot in
            // front of the name and no way to start it.
            let hidden = path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with('.'));
            if hidden {
                continue;
            }
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
    skipped.sort();
    Ok((out, skipped))
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

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("romm-esde-test-{name}"));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    pub(super) fn touch(path: &Path, body: &[u8]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }

    pub(super) fn map() -> CoreMap {
        serde_json::from_str(
            r#"{
              "default_core_by_romm_platform": {"snes": "snes9x", "megadrive": "genesisgx"},
              "systems": {
                "snes":      {"romm_platforms": ["snes"],      "emulators": []},
                "megadrive": {"romm_platforms": ["megadrive"], "emulators": []}
              }
            }"#,
        )
        .unwrap()
    }

    /// The KNULLI scheme, end to end through a real scan.
    ///
    /// Measured on the Flip on 2026-08-24: `fbneo` holds 2,504 zips and is the
    /// arcade library; `wswan`, `wswanc` and `gamecube` are empty and not
    /// wanted. Without the scheme's alias the whole arcade set scans to
    /// nothing, and `esde::scan` reports that as a skipped directory rather
    /// than as an error — which is exactly why it went unnoticed.
    #[cfg(feature = "knulli")]
    #[test]
    fn on_knulli_fbneo_is_the_arcade_library_and_the_unwanted_are_silent() {
        let root = scratch("knulli-scan");
        let roms = root.join("roms");
        touch(&roms.join("fbneo/sf2.zip"), b"rom");
        touch(&roms.join("wswan/gunpey.ws"), b"rom");
        touch(&roms.join("gamecube/melee.iso"), b"rom");
        touch(&roms.join("nowhere/mystery.bin"), b"rom");

        let layout = Layout::new(&root, Some(&roms));
        let (games, skipped) = scan(&layout, &map()).unwrap();

        assert_eq!(
            games.iter().map(|g| g.platform_slug.as_str()).collect::<Vec<_>>(),
            ["arcade"],
            "fbneo is the arcade library and nothing else should have scanned"
        );
        assert!(
            !skipped.iter().any(|s| s == "wswan" || s == "gamecube"),
            "hidden means silent, not reported: {skipped:?}"
        );
        assert!(
            skipped.iter().any(|s| s == "nowhere"),
            "a genuinely unknown directory must still be reported, or the next \
             missing alias is invisible too: {skipped:?}"
        );
    }

    /// ES-DE keeps its ROMs directory separate and configurable, so it is not
    /// safe to assume `<root>/ROMs`.
    #[test]
    fn an_explicit_roms_directory_overrides_the_default() {
        let default = Layout::new(Path::new("/esde"), None);
        assert_eq!(default.roms, Path::new("/esde/ROMs"));
        assert_eq!(default.gamelists, Path::new("/esde/gamelists"));
        assert_eq!(default.media, Path::new("/esde/downloaded_media"));

        let custom = Layout::new(Path::new("/esde"), Some(Path::new("/Volumes/SD/games")));
        assert_eq!(custom.roms, Path::new("/Volumes/SD/games"));
        assert_eq!(
            custom.gamelists,
            Path::new("/esde/gamelists"),
            "only the ROMs location moves"
        );
    }

    /// The gamelist is the only source of real titles. Rating is rescaled from
    /// ES-DE's 0.0–1.0 to RomM's 0–100, and the year is the first four
    /// characters of a timestamp — both silent if wrong.
    #[test]
    fn a_gamelist_entry_is_parsed_into_metadata() {
        let dir = scratch("gamelist");
        let path = dir.join("gamelist.xml");
        touch(
            &path,
            br#"<?xml version="1.0"?>
            <gameList>
              <game>
                <path>./Chrono Trigger (USA).sfc</path>
                <name>Chrono Trigger</name>
                <desc>A tale of time travel &amp; friendship</desc>
                <genre>Role-Playing</genre>
                <players>1</players>
                <rating>0.95</rating>
                <releasedate>19950811T000000</releasedate>
              </game>
            </gameList>"#,
        );

        let meta = parse_gamelist(&path);
        let e = meta.get("Chrono Trigger (USA)").expect("keyed by file stem");
        assert_eq!(e.name.as_deref(), Some("Chrono Trigger"));
        assert_eq!(
            e.desc.as_deref(),
            Some("A tale of time travel & friendship"),
            "entities must be unescaped"
        );
        assert_eq!(e.rating, Some(95.0), "0.95 becomes 95, not 0.95");
        assert_eq!(e.year, Some(1995));
        assert_eq!(e.players.as_deref(), Some("1"));
    }

    /// `<path>` is relative and prefixed in a couple of ways; all of them have
    /// to reduce to the same key or the metadata never joins to the file.
    #[test]
    fn game_paths_reduce_to_a_file_stem_however_they_are_written() {
        let dir = scratch("gamelist-paths");
        let path = dir.join("gamelist.xml");
        touch(
            &path,
            br#"<gameList>
              <game><path>./Sonic.md</path><name>Dotted</name></game>
              <game><path>Sub/Dir/Streets of Rage.md</path><name>Nested</name></game>
              <game><path>.\Windows\Golden Axe.md</path><name>Backslashes</name></game>
            </gameList>"#,
        );
        let meta = parse_gamelist(&path);
        assert_eq!(meta.get("Sonic").and_then(|e| e.name.clone()).as_deref(), Some("Dotted"));
        assert_eq!(
            meta.get("Streets of Rage").and_then(|e| e.name.clone()).as_deref(),
            Some("Nested")
        );
        assert_eq!(
            meta.get("Golden Axe").and_then(|e| e.name.clone()).as_deref(),
            Some("Backslashes")
        );
    }

    /// A game with almost no metadata must not poison the whole gamelist, and
    /// empty tags count as absent rather than as an empty title.
    #[test]
    fn sparse_and_empty_fields_are_treated_as_missing() {
        let dir = scratch("gamelist-sparse");
        let path = dir.join("gamelist.xml");
        touch(
            &path,
            br#"<gameList>
              <game><path>./A.sfc</path><name></name><rating></rating></game>
              <game><path>./B.sfc</path><name>Has a name</name></game>
            </gameList>"#,
        );
        let meta = parse_gamelist(&path);
        assert!(meta.get("A").unwrap().name.is_none(), "an empty tag is not a title");
        assert!(meta.get("A").unwrap().rating.is_none());
        assert_eq!(meta.get("B").unwrap().name.as_deref(), Some("Has a name"));
    }

    /// A missing gamelist is ordinary — plenty of systems have none — and must
    /// yield no metadata rather than failing the scan.
    #[test]
    fn a_missing_gamelist_is_not_an_error() {
        assert!(parse_gamelist(Path::new("/nonexistent/gamelist.xml")).is_empty());
    }

    /// Saves, configs and dotfiles sit beside games in an ES-DE tree. Indexing
    /// them invents games that cannot launch.
    #[test]
    fn non_game_files_beside_the_roms_are_not_indexed() {
        for name in ["gamelist.xml", "notes.txt", "Zelda.srm", "Zelda.state", "es.cfg", ".DS_Store"] {
            assert!(!is_game_file(Path::new(name)), "{name} is not a game");
        }
        for name in ["Zelda.sfc", "Sonic.md", "Game.chd", "Game.zip"] {
            assert!(is_game_file(Path::new(name)), "{name} is a game");
        }
    }

    /// The alias that matters most: a real install calls it `genesis` where the
    /// map says `megadrive`. Without the alias this drops the single largest
    /// system on the card.
    #[test]
    fn a_real_installs_system_names_are_aliased_to_the_map() {
        let dir = scratch("scan-alias");
        touch(&dir.join("ROMs/genesis/Sonic.md"), b"rom");
        let layout = Layout::new(&dir, None);

        let (games, skipped) = scan(&layout, &map()).unwrap();
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].platform_slug, "megadrive", "genesis is megadrive");
        assert_eq!(games[0].system, "genesis", "but its media still lives under genesis");
        assert!(skipped.is_empty());
    }

    /// A BIOS folder scanned as a system invents phantom games and hands them
    /// to a core that cannot run them.
    #[test]
    fn bios_and_ports_directories_are_not_systems() {
        let dir = scratch("scan-bios");
        touch(&dir.join("ROMs/snes/Zelda.sfc"), b"rom");
        touch(&dir.join("ROMs/0_BIOS/scph1001.bin"), b"bios");
        touch(&dir.join("ROMs/bios/neogeo.zip"), b"bios");
        touch(&dir.join("ROMs/ports/doom.sh"), b"port");
        touch(&dir.join("ROMs/.hidden/x.bin"), b"junk");
        let layout = Layout::new(&dir, None);

        let (games, skipped) = scan(&layout, &map()).unwrap();
        assert_eq!(games.len(), 1, "only the real system");
        assert_eq!(games[0].name, "Zelda");
        assert!(
            !skipped.iter().any(|s| s == "0_BIOS" || s == "bios" || s == "ports"),
            "these are excluded outright, not reported as unmapped systems"
        );
    }

    /// An unknown system is reported rather than guessed at: a wrong slug puts
    /// games under a platform whose core cannot run them.
    #[test]
    fn an_unmapped_system_is_skipped_and_named() {
        let dir = scratch("scan-unknown");
        touch(&dir.join("ROMs/snes/Zelda.sfc"), b"rom");
        touch(&dir.join("ROMs/vectrex/Minestorm.bin"), b"rom");
        let layout = Layout::new(&dir, None);

        let (games, skipped) = scan(&layout, &map()).unwrap();
        assert_eq!(games.len(), 1);
        assert_eq!(skipped, ["vectrex"], "reported so the user can add a mapping");
    }

    /// A directory inside a system is a multi-disc game, which ES-DE treats as
    /// one entry — the same shape as RomM's folder ROMs.
    #[test]
    fn a_multi_disc_directory_counts_as_one_game_and_is_sized_whole() {
        let dir = scratch("scan-folder");
        touch(&dir.join("ROMs/snes/Solo.sfc"), b"12345");
        touch(&dir.join("ROMs/snes/Shenmue (USA)/disc1.chd"), b"aaa");
        touch(&dir.join("ROMs/snes/Shenmue (USA)/disc2.chd"), b"bb");
        let layout = Layout::new(&dir, None);

        let (games, _) = scan(&layout, &map()).unwrap();
        assert_eq!(games.len(), 2, "the folder is one game, not two files");
        let folder = games.iter().find(|g| g.fs_name == "Shenmue (USA)").expect("folder game");
        assert_eq!(folder.size_bytes, 5, "summed across the folder's contents");
        let solo = games.iter().find(|g| g.fs_name == "Solo.sfc").unwrap();
        assert_eq!(solo.size_bytes, 5);
    }

    /// The gamelist supplies the title; without one the file stem stands in, so
    /// a game is never nameless.
    #[test]
    fn scanned_games_take_their_title_from_the_gamelist_or_the_filename() {
        let dir = scratch("scan-names");
        touch(&dir.join("ROMs/snes/ct.sfc"), b"rom");
        touch(&dir.join("ROMs/snes/unnamed.sfc"), b"rom");
        touch(
            &dir.join("gamelists/snes/gamelist.xml"),
            br#"<gameList><game><path>./ct.sfc</path><name>Chrono Trigger</name>
                <genre>RPG, Adventure</genre></game></gameList>"#,
        );
        let layout = Layout::new(&dir, None);

        let (games, _) = scan(&layout, &map()).unwrap();
        let named = games.iter().find(|g| g.fs_name == "ct.sfc").unwrap();
        assert_eq!(named.name, "Chrono Trigger");
        assert_eq!(named.genres, ["RPG", "Adventure"], "split and trimmed");
        let plain = games.iter().find(|g| g.fs_name == "unnamed.sfc").unwrap();
        assert_eq!(plain.name, "unnamed", "falls back to the stem");
    }

    /// A missing ROMs directory is worth saying plainly — it is the single most
    /// likely thing to be misconfigured.
    #[test]
    fn scanning_without_a_roms_directory_says_so() {
        let dir = scratch("scan-noroms");
        let layout = Layout::new(&dir, None);
        let err = scan(&layout, &map()).expect_err("no ROMs dir").to_string();
        assert!(err.contains("ROMs"), "got: {err}");
    }

    /// Media is keyed by ES-DE system name and found by probing extensions,
    /// because ES-DE stores whatever the scraper produced.
    #[test]
    fn media_is_located_by_probing_the_extensions_esde_uses() {
        let dir = scratch("media");
        let layout = Layout::new(&dir, None);
        touch(&dir.join("downloaded_media/snes/covers/Zelda.jpg"), b"img");

        assert_eq!(
            media_path(&layout, "snes", "Zelda", "covers"),
            Some(dir.join("downloaded_media/snes/covers/Zelda.jpg"))
        );
        assert_eq!(media_path(&layout, "snes", "Zelda", "videos"), None);
        assert_eq!(media_path(&layout, "snes", "Missing", "covers"), None);
    }
}

#[cfg(test)]
mod hidden {
    use super::tests::{map, scratch, touch};
    use super::*;

    /// A leading dot means hidden, and the device means it.
    ///
    /// Batocera files multi-disc games as `.Final Fantasy VII (USA)/` with the
    /// `.m3u` beside it, and its own front end skips the folder. Scanning it
    /// anyway listed every one of those games twice — once properly, once with
    /// a dot in front of the name and no way to start it.
    #[test]
    fn hidden_entries_are_not_games() {
        let dir = scratch("hidden");
        let roms = dir.join("roms");
        let psx = roms.join("snes");
        std::fs::create_dir_all(&psx).unwrap();
        touch(&psx.join("Super Mario World (USA).sfc"), b"x");
        touch(&psx.join("Chrono Trigger (USA).sfc"), b"x");
        std::fs::create_dir_all(psx.join(".Chrono Trigger (USA)")).unwrap();
        touch(&psx.join(".Chrono Trigger (USA)/disc1.sfc"), b"x");
        touch(&psx.join(".hidden thing.sfc"), b"x");

        let layout = Layout::new(&dir, Some(&roms));
        let (games, _) = scan(&layout, &map()).unwrap();
        let mut names: Vec<&str> = games.iter().map(|g| g.fs_name.as_str()).collect();
        names.sort();
        assert_eq!(
            names,
            ["Chrono Trigger (USA).sfc", "Super Mario World (USA).sfc"],
            "a hidden entry was scanned"
        );
    }
}
