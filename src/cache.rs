//! Local metadata cache.
//!
//! A full cold pull is only ~8 seconds (PLAN.md §3), so this exists for offline
//! browsing and instant navigation rather than to work around slowness. After
//! the first sync it goes incremental via `updated_after`.

use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::{Connection, params};

use crate::api;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS platforms (
    id            INTEGER PRIMARY KEY,
    fs_slug       TEXT NOT NULL UNIQUE,
    display_name  TEXT,
    rom_count     INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS roms (
    id             INTEGER PRIMARY KEY,
    platform_slug  TEXT NOT NULL,
    name           TEXT,
    fs_name        TEXT NOT NULL,
    fs_size_bytes  INTEGER,
    md5_hash       TEXT,
    sha1_hash      TEXT,
    crc_hash       TEXT,
    updated_at     TEXT,
    cover_path     TEXT,
    screenshot_path TEXT,
    screenshots_json TEXT,
    cover_small_path TEXT,
    summary        TEXT,
    meta_json      TEXT,
    alt_names_json TEXT,
    regions_json   TEXT,
    manual_path    TEXT,
    youtube_id     TEXT,
    multi_file     INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS roms_platform ON roms(platform_slug);
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
-- Collections mirror the server rather than being a local invention: this is a
-- RomM client, so whatever RomM groups games into is what we show.
CREATE TABLE IF NOT EXISTS collections (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    grp         TEXT NOT NULL,
    description TEXT,
    rom_count   INTEGER NOT NULL DEFAULT 0,
    is_favorite INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS collection_roms (
    collection_id TEXT NOT NULL,
    rom_id        INTEGER NOT NULL,
    PRIMARY KEY (collection_id, rom_id)
);
CREATE INDEX IF NOT EXISTS collection_roms_rom ON collection_roms(rom_id);
-- Every session, one row.
--
-- Kept here rather than on the game because the interesting questions are about
-- the shape of the sessions, not their sum: a game opened eleven times for four
-- minutes each is a different thing from one played twice for an afternoon, and
-- a single "hours played" column cannot tell them apart. The server has no
-- equivalent, so this is the only record there is.
CREATE TABLE IF NOT EXISTS plays (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    rom_id     INTEGER NOT NULL,
    started_at TEXT NOT NULL,
    seconds    INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS plays_rom ON plays(rom_id);
CREATE INDEX IF NOT EXISTS collections_grp ON collections(grp);
"#;

pub struct Cache {
    conn: Connection,
}

#[derive(Debug, Clone)]
pub struct CollectionRow {
    pub id: String,
    pub name: String,
    /// `user`, `smart`, or a virtual kind such as `genre` / `franchise`.
    pub group: String,
    pub description: Option<String>,
    pub rom_count: i64,
    pub is_favorite: bool,
    /// A few member ROM ids, so the card can show real cover art through the
    /// same local cache the game grids use.
    pub sample_ids: Vec<i64>,
}

#[derive(Debug, Clone)]
pub struct PlatformRow {
    pub fs_slug: String,
    pub display_name: String,
    pub rom_count: i64,
}

#[derive(Debug, Clone, Default)]
pub struct RomRow {
    /// Used for `/api/roms/{id}/content/{fs_name}`.
    pub id: i64,
    pub platform_slug: String,
    pub name: String,
    pub fs_name: String,
    pub fs_size_bytes: i64,
    pub md5_hash: Option<String>,
    pub sha1_hash: Option<String>,
    /// Server-relative artwork paths, if the server has any.
    pub cover_path: Option<String>,
    pub screenshot_path: Option<String>,
    /// Every screenshot the server has, JSON-encoded. Games range from 0 to 12.
    pub screenshots_json: Option<String>,
    pub cover_small_path: Option<String>,
    pub summary: Option<String>,
    /// RomM's merged metadata: genres, companies, player count, rating…
    pub meta_json: Option<String>,
    pub alt_names_json: Option<String>,
    pub regions_json: Option<String>,
    pub manual_path: Option<String>,
    pub youtube_id: Option<String>,
    pub multi_file: bool,
    /// ES-DE system directory this came from, when the library was scanned
    /// from a local ES-DE tree. Artwork there is keyed by ES-DE system name,
    /// not by RomM slug, so the two cannot be used interchangeably.
    pub esde_system: Option<String>,
    /// Absolute path for a locally scanned game.
    pub local_path: Option<String>,
}

/// Columns every `RomRow` query selects, in order.
/// Every game in a starred collection.
///
/// Two ways in, because there are two ways to star something. RomM flags a
/// collection it considers a favourite, and a person marks one by putting a
/// star in the name — which is what happened on this library. Reading both
/// means the app agrees with whichever the user did.
const FAVOURITE_ROMS: &str = "SELECT cr.rom_id FROM collection_roms cr \
                              JOIN collections c ON c.id = cr.collection_id \
                              WHERE c.is_favorite = 1 OR c.name LIKE '★%'";

const ROM_COLUMNS: &str = "id, platform_slug, COALESCE(NULLIF(name, ''), fs_name), \
                           fs_name, COALESCE(fs_size_bytes, 0), md5_hash, sha1_hash, \
                           cover_path, screenshot_path, screenshots_json, \
                           cover_small_path, summary, meta_json, alt_names_json, \
                           regions_json, manual_path, youtube_id, \
                           COALESCE(multi_file, 0), esde_system, local_path";

fn rom_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<RomRow> {
    Ok(RomRow {
        id: r.get(0)?,
        platform_slug: r.get(1)?,
        name: r.get(2)?,
        fs_name: r.get(3)?,
        fs_size_bytes: r.get(4)?,
        md5_hash: r.get(5)?,
        sha1_hash: r.get(6)?,
        cover_path: r.get(7)?,
        screenshot_path: r.get(8)?,
        screenshots_json: r.get(9)?,
        cover_small_path: r.get(10)?,
        summary: r.get(11)?,
        meta_json: r.get(12)?,
        alt_names_json: r.get(13)?,
        regions_json: r.get(14)?,
        manual_path: r.get(15)?,
        youtube_id: r.get(16)?,
        // Read tolerantly: the migration adds columns as TEXT, so an older
        // cache stores this as "0"/"1" while a freshly created one stores an
        // integer. Both must work without forcing a rebuild.
        multi_file: match r.get_ref(17)? {
            rusqlite::types::ValueRef::Integer(i) => i != 0,
            rusqlite::types::ValueRef::Text(b) => {
                !matches!(std::str::from_utf8(b).unwrap_or("0"), "0" | "" | "false")
            }
            _ => false,
        },
        esde_system: r.get(18)?,
        local_path: r.get(19)?,
    })
}

impl RomRow {
    /// Server-side screenshot paths, newest schema first, falling back to the
    /// single-path column for caches written before the list was stored.
    pub fn screenshots(&self) -> Vec<String> {
        if let Some(json) = &self.screenshots_json
            && let Ok(v) = serde_json::from_str::<Vec<String>>(json)
            && !v.is_empty()
        {
            return v;
        }
        self.screenshot_path.clone().into_iter().collect()
    }
}

impl Cache {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent()
            && !dir.as_os_str().is_empty()
        {
            std::fs::create_dir_all(dir).ok();
        }
        let conn = Connection::open(path)
            .with_context(|| format!("opening cache at {}", path.display()))?;
        conn.execute_batch(SCHEMA).context("creating schema")?;
        // Older caches predate the artwork columns; add them in place rather
        // than forcing a full resync.
        for (col, ty) in [
            ("cover_path", "TEXT"), ("screenshot_path", "TEXT"),
            ("screenshots_json", "TEXT"), ("cover_small_path", "TEXT"),
            ("summary", "TEXT"), ("meta_json", "TEXT"), ("alt_names_json", "TEXT"),
            ("regions_json", "TEXT"), ("manual_path", "TEXT"),
            ("youtube_id", "TEXT"), ("multi_file", "INTEGER NOT NULL DEFAULT 0"),
            // ES-DE libraries live wherever the user put them, so a game's
            // location cannot be derived from <roms>/<slug>/<fs_name>.
            ("local_path", "TEXT"), ("esde_system", "TEXT"),
            // When the server last saw this game played. Drives the row of
            // recent games, and comes from the server rather than being
            // recorded here, so it follows you between machines.
            ("last_played", "TEXT"),
        ] {
            let _ = conn.execute(&format!("ALTER TABLE roms ADD COLUMN {col} {ty}"), []);
        }
        Ok(Self { conn })
    }

    /// Replace the stored collections wholesale.
    ///
    /// Virtual collections are recomputed by the server from scratch and their
    /// ids are derived from name+type, so a rename silently orphans the old
    /// row. Rebuilding is cheaper and more correct than reconciling.
    pub fn replace_collections(&mut self, items: &[api::Collection]) -> Result<usize> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM collection_roms", [])?;
        tx.execute("DELETE FROM collections", [])?;
        {
            let mut ins = tx.prepare(
                "INSERT OR REPLACE INTO collections
                 (id, name, grp, description, rom_count, is_favorite)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            let mut link = tx.prepare(
                "INSERT OR IGNORE INTO collection_roms(collection_id, rom_id) VALUES (?1, ?2)",
            )?;
            for c in items {
                // Trust the member list over the server's count: the two
                // disagree when a member rom has since been deleted.
                ins.execute(params![
                    c.id,
                    c.name,
                    c.group(),
                    c.description,
                    c.rom_ids.len() as i64,
                    c.is_favorite as i64,
                ])?;
                for rom_id in &c.rom_ids {
                    link.execute(params![c.id, rom_id])?;
                }
            }
        }
        tx.commit()?;
        Ok(items.len())
    }

    /// Collection groups present, with how many collections each holds.
    ///
    /// Counts only collections that still have at least one ROM we know about,
    /// so a group cannot advertise entries that open empty.
    pub fn collection_groups(&self) -> Result<Vec<(String, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT c.grp, COUNT(*) FROM collections c
             WHERE EXISTS (SELECT 1 FROM collection_roms cr JOIN roms r ON r.id = cr.rom_id
                           WHERE cr.collection_id = c.id)
             GROUP BY c.grp ORDER BY COUNT(*) DESC",
        )?;
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Collections in one group, largest first, skipping any that would open
    /// empty against the ROMs actually in the cache.
    pub fn collections_in(&self, group: &str) -> Result<Vec<CollectionRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT c.id, c.name, c.grp, c.description,
                    (SELECT COUNT(*) FROM collection_roms cr JOIN roms r ON r.id = cr.rom_id
                     WHERE cr.collection_id = c.id) AS live,
                    c.is_favorite,
                    (SELECT group_concat(rom_id) FROM
                       (SELECT cr.rom_id FROM collection_roms cr JOIN roms r ON r.id = cr.rom_id
                        WHERE cr.collection_id = c.id LIMIT 4))
             FROM collections c
             WHERE c.grp = ?1 AND live > 0
             ORDER BY live DESC, c.name COLLATE NOCASE",
        )?;
        let rows = stmt
            .query_map([group], |r| {
                Ok(CollectionRow {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    group: r.get(2)?,
                    description: r.get(3)?,
                    rom_count: r.get(4)?,
                    is_favorite: r.get::<_, i64>(5)? != 0,
                    sample_ids: r
                        .get::<_, Option<String>>(6)?
                        .unwrap_or_default()
                        .split(',')
                        .filter_map(|s| s.parse().ok())
                        .collect(),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// ROMs belonging to one collection.
    /// Every game in a collection group, each listed once.
    ///
    /// `DISTINCT` matters: the "My collections" group holds overlapping lists —
    /// a game can be in both Arcade Fighting and Arcade Essentials — and
    /// without it the same download would be queued twice.
    pub fn roms_in_group(&self, grp: &str) -> Result<Vec<RomRow>> {
        // A subquery rather than a join: `collections` carries `id`, `name`
        // and `description` too, and joining it puts those in scope alongside
        // the same names on `roms`, which SQLite rejects as ambiguous.
        let sql = format!(
            "SELECT {ROM_COLUMNS} FROM roms WHERE id IN ( \
                 SELECT cr.rom_id FROM collection_roms cr \
                 JOIN collections c ON c.id = cr.collection_id \
                 WHERE c.grp = ?1) \
             ORDER BY 2, 3 COLLATE NOCASE"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map([grp], rom_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn roms_in_collection(&self, id: &str) -> Result<Vec<RomRow>> {
        let sql = format!(
            "SELECT {ROM_COLUMNS} FROM roms r
             JOIN collection_roms cr ON cr.rom_id = r.id
             WHERE cr.collection_id = ?1
             ORDER BY COALESCE(NULLIF(r.name, ''), r.fs_name) COLLATE NOCASE"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map([id], rom_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Replace bare romset names with the real titles from the DAT map.
    ///
    /// Run after a sync, because a sync rewrites `name` from the server and
    /// would otherwise put `kof98` back.
    pub fn apply_arcade_names(
        &mut self,
        names: &std::collections::BTreeMap<String, String>,
    ) -> Result<usize> {
        if names.is_empty() {
            return Ok(0);
        }
        let rows: Vec<(i64, String, String)> = {
            let mut stmt = self.conn.prepare(
                "SELECT id, COALESCE(name, ''), fs_name FROM roms WHERE platform_slug IN
                 (SELECT value FROM json_each(?1))",
            )?;
            let list = serde_json::to_string(crate::arcade::ARCADE_PLATFORMS)?;
            stmt.query_map([list], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };

        let tx = self.conn.transaction()?;
        let mut n = 0;
        {
            let mut up = tx.prepare("UPDATE roms SET name = ?1 WHERE id = ?2")?;
            for (id, name, fs_name) in rows {
                if !crate::arcade::is_bare_romset(&name, &fs_name) {
                    continue;
                }
                let stem = fs_name.rsplit_once('.').map_or(fs_name.as_str(), |(s, _)| s);
                if let Some(title) = names.get(stem)
                    && !title.eq_ignore_ascii_case(&name)
                {
                    up.execute(params![title, id])?;
                    n += 1;
                }
            }
        }
        tx.commit()?;
        Ok(n)
    }

    /// Replace the library with what was found in a local ES-DE install.
    ///
    /// Wholesale rather than incremental: there is no server watermark to
    /// diff against, and a local scan is fast enough that reconciling would
    /// be more code for no gain. Collections are left untouched — they belong
    /// to RomM and mean nothing here.
    pub fn replace_from_esde(&mut self, games: &[crate::esde::Game]) -> Result<usize> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM roms", [])?;
        tx.execute("DELETE FROM platforms", [])?;
        {
            let mut ins = tx.prepare(
                "INSERT INTO roms (id, platform_slug, name, fs_name, fs_size_bytes,
                                   summary, meta_json, local_path, esde_system, multi_file)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            )?;
            let mut plat = tx.prepare(
                "INSERT OR REPLACE INTO platforms (id, fs_slug, display_name, rom_count)
                 VALUES (?1, ?2, ?3, ?4)",
            )?;
            let mut counts: std::collections::BTreeMap<&str, i64> = Default::default();

            for (i, g) in games.iter().enumerate() {
                // Local ids are positional; nothing here refers to RomM's.
                let id = i as i64 + 1;
                let meta = serde_json::json!({
                    "genres": g.genres,
                    "player_count": g.players,
                    "average_rating": g.rating,
                    "first_release_date": g.release_year,
                });
                ins.execute(params![
                    id,
                    g.platform_slug,
                    g.name,
                    g.fs_name,
                    g.size_bytes,
                    g.summary,
                    meta.to_string(),
                    g.path.to_string_lossy(),
                    g.system,
                    i64::from(g.path.is_dir()),
                ])?;
                *counts.entry(g.platform_slug.as_str()).or_default() += 1;
            }
            for (i, (slug, n)) in counts.iter().enumerate() {
                plat.execute(params![i as i64 + 1, slug, slug, n])?;
            }
        }
        tx.commit()?;
        Ok(games.len())
    }

    pub fn collection_count(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM collections", [], |r| r.get(0))?)
    }

    fn meta_get(&self, key: &str) -> Option<String> {
        self.conn
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
            .ok()
    }

    fn meta_set(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO meta(key, value) VALUES(?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// Remember server settings that change how we interpret its data, so a
    /// download verifies identically when the server is unreachable.
    pub fn save_server_config(&self, cfg: &api::ServerConfig) -> Result<()> {
        self.meta_set("excluded_files", &serde_json::to_string(&cfg.default_excluded_files)?)?;
        self.meta_set("excluded_exts", &serde_json::to_string(&cfg.default_excluded_extensions)?)?;
        self.meta_set("skip_hash", &cfg.skip_hash_calculation.to_string())?;
        Ok(())
    }

    /// `(excluded_files, excluded_extensions)` as last seen, if ever fetched.
    pub fn server_exclusions(&self) -> Option<(Vec<String>, Vec<String>)> {
        let files = serde_json::from_str(&self.meta_get("excluded_files")?).ok()?;
        let exts = serde_json::from_str(&self.meta_get("excluded_exts")?).ok()?;
        Some((files, exts))
    }

    pub fn server_version(&self) -> Option<String> {
        self.meta_get("server_version")
    }

    pub fn set_server_version(&self, v: &str) -> Result<()> {
        self.meta_set("server_version", v)
    }

    /// High-water mark of `updated_at` across everything we've stored.
    ///
    /// Using the max row timestamp rather than "now" avoids losing rows to
    /// clock skew between this machine and the server.
    pub fn watermark(&self) -> Option<String> {
        self.meta_get("roms_updated_through")
    }

    pub fn rom_count(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM roms", [], |r| r.get(0))?)
    }

    pub fn platforms(&self) -> Result<Vec<PlatformRow>> {
        // Count from the roms we actually hold, so the UI never promises rows
        // it cannot show.
        let mut stmt = self.conn.prepare(
            "SELECT p.fs_slug,
                    COALESCE(NULLIF(p.display_name, ''), p.fs_slug),
                    (SELECT COUNT(*) FROM roms r WHERE r.platform_slug = p.fs_slug)
             FROM platforms p
             -- By display name. It was ordered by ROM count, which put arcade
             -- and megadrive first and scattered everything else with no
             -- visible logic; alphabetical means a console is where you expect
             -- it. COLLATE NOCASE so casing does not split the order.
             ORDER BY 2 COLLATE NOCASE ASC",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(PlatformRow {
                    fs_slug: r.get(0)?,
                    display_name: r.get(1)?,
                    rom_count: r.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows.into_iter().filter(|p| p.rom_count > 0).collect())
    }

    /// Games you have starred, as a set of ids.
    ///
    /// RomM has no per-game favourite of its own — a favourite there is a
    /// *collection*, either one the server has flagged or one you named with a
    /// star, which is what the "★ Best of …" collections on this library are.
    /// So a game counts as a favourite when it is in one of those, and this
    /// stays true whether the starring happened here or on the web.
    pub fn favourite_ids(&self) -> Result<std::collections::HashSet<i64>> {
        let mut stmt = self.conn.prepare(FAVOURITE_ROMS)?;
        let ids = stmt
            .query_map([], |r| r.get::<_, i64>(0))?
            .collect::<Result<std::collections::HashSet<_>, _>>()?;
        Ok(ids)
    }

    /// The games played most recently, newest first.
    ///
    /// Server-side timestamps, so this is the same list on every machine — the
    /// point of it is picking up where you left off, and "where you left off"
    /// is rarely the machine you are now sitting at.
    pub fn recently_played(&self, limit: usize) -> Result<Vec<RomRow>> {
        let sql = format!(
            "SELECT {ROM_COLUMNS} FROM roms \
             WHERE last_played IS NOT NULL AND last_played <> '' \
             ORDER BY last_played DESC LIMIT ?1"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map([limit as i64], rom_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Record one finished session, and mark the game played.
    ///
    /// `last_played` is also set locally. It used to come only from the server,
    /// which meant playing a game on this machine changed nothing on the
    /// "continue playing" row until a sync happened to bring back a timestamp
    /// the server had no reason to have — so the row was often a list of what
    /// somebody else's machine had been doing.
    ///
    /// Sessions under a minute are dropped. Starting a game and quitting
    /// straight back out is a thing people do constantly — wrong game, wrong
    /// controller, checking it runs — and counting those makes "eleven
    /// sessions" mean nothing.
    pub fn record_play(&self, rom_id: i64, started_at: &str, seconds: i64) -> Result<bool> {
        if seconds < 60 {
            return Ok(false);
        }
        self.conn.execute(
            "INSERT INTO plays(rom_id, started_at, seconds) VALUES (?1, ?2, ?3)",
            rusqlite::params![rom_id, started_at, seconds],
        )?;
        self.conn.execute(
            "UPDATE roms SET last_played = ?1 WHERE id = ?2",
            rusqlite::params![started_at, rom_id],
        )?;
        Ok(true)
    }

    /// Time played per console, longest first.
    pub fn play_by_platform(&self) -> Result<Vec<(String, i64, i64, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT r.platform_slug, SUM(p.seconds), COUNT(*), COUNT(DISTINCT p.rom_id)              FROM plays p JOIN roms r ON r.id = p.rom_id              GROUP BY r.platform_slug ORDER BY 2 DESC",
        )?;
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Time played per game, longest first: `(rom, seconds, sessions, last)`.
    pub fn play_by_game(&self, limit: usize) -> Result<Vec<(RomRow, i64, i64, String)>> {
        let sql = format!(
            "SELECT {ROM_COLUMNS}, t.secs, t.runs, t.last FROM roms              JOIN (SELECT rom_id, SUM(seconds) secs, COUNT(*) runs, MAX(started_at) last                    FROM plays GROUP BY rom_id) t ON t.rom_id = roms.id              ORDER BY t.secs DESC LIMIT ?1"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        // By name, not by position. ROM_COLUMNS holds a COALESCE with commas
        // inside it, so counting separators to find where the extra columns
        // start gives a number several too high.
        let rows = stmt
            .query_map([limit as i64], |r| {
                Ok((rom_from_row(r)?, r.get("secs")?, r.get("runs")?, r.get("last")?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Games picked up more than once and never really played.
    ///
    /// The definition is deliberately narrow: opened on at least `runs`
    /// separate occasions, and under `under` seconds in total. Something you
    /// came back to and still bounced off — which is a more interesting list
    /// than "games you started once", because that one is just your library.
    pub fn abandoned(&self, runs: i64, under: i64, limit: usize) -> Result<Vec<(RomRow, i64, i64)>> {
        let sql = format!(
            "SELECT {ROM_COLUMNS}, t.secs, t.runs FROM roms              JOIN (SELECT rom_id, SUM(seconds) secs, COUNT(*) runs FROM plays                    GROUP BY rom_id) t ON t.rom_id = roms.id              WHERE t.runs >= ?1 AND t.secs < ?2 ORDER BY t.runs DESC, t.secs ASC LIMIT ?3"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(rusqlite::params![runs, under, limit as i64], |r| {
                Ok((rom_from_row(r)?, r.get("secs")?, r.get("runs")?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Total seconds and session count across everything.
    pub fn play_totals(&self) -> Result<(i64, i64, i64)> {
        Ok(self.conn.query_row(
            "SELECT COALESCE(SUM(seconds), 0), COUNT(*), COUNT(DISTINCT rom_id) FROM plays",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?)
    }

    /// Every game, ordered as the platform pages order them.
    pub fn all_roms(&self) -> Result<Vec<RomRow>> {
        let sql = format!("SELECT {ROM_COLUMNS} FROM roms ORDER BY 2, 3 COLLATE NOCASE");
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([], rom_from_row)?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn roms_for(&self, platform_slug: &str) -> Result<Vec<RomRow>> {
        // Favourites first, then alphabetical within each group. A console
        // page is a wall of a few hundred names; the handful you actually play
        // being at the top is the difference between browsing and searching.
        let sql = format!(
            "SELECT {ROM_COLUMNS} FROM roms WHERE platform_slug = ?1 \
             ORDER BY (id IN ({FAVOURITE_ROMS})) DESC, 3 COLLATE NOCASE"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map([platform_slug], rom_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// The platform a file on disk belongs to, by exact path.
    ///
    /// Path inference expects `<roms>/<slug>/<file>`, which an ES-DE library
    /// does not satisfy: its directories are ES-DE system names (`dreamcast`,
    /// `neogeo`), not RomM slugs. Asking the index is exact and works for any
    /// layout.
    pub fn platform_for_path(&self, path: &Path) -> Option<String> {
        let p = path.to_string_lossy().to_string();
        self.conn
            .query_row(
                "SELECT platform_slug FROM roms WHERE local_path = ?1 LIMIT 1",
                [&p],
                |r| r.get(0),
            )
            .ok()
    }

    pub fn rom_by_id(&self, id: i64) -> Result<Option<RomRow>> {
        let sql = format!("SELECT {ROM_COLUMNS} FROM roms WHERE id = ?1");
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query_map([id], rom_from_row)?;
        Ok(match rows.next() {
            Some(r) => Some(r?),
            None => None,
        })
    }

    /// Case-insensitive search over display name and filename.
    pub fn search(&self, needle: &str, limit: usize) -> Result<Vec<RomRow>> {
        let sql = format!(
            "SELECT {ROM_COLUMNS} FROM roms \
             WHERE name LIKE ?1 OR fs_name LIKE ?1 \
             ORDER BY 3 COLLATE NOCASE LIMIT ?2"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let pattern = format!("%{needle}%");
        let rows = stmt
            .query_map(params![pattern, limit as i64], rom_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Drop cached roms the server no longer has.
    ///
    /// Incremental sync only ever learns about additions and changes, so
    /// without this a deleted rom lingers forever — as happened when 18
    /// multi-disc playlist stubs were replaced by folder ROMs and both showed
    /// up in the UI.
    pub fn prune_missing(&mut self, live_ids: &[i64]) -> Result<usize> {
        if live_ids.is_empty() {
            return Ok(0);
        }
        let tx = self.conn.transaction()?;
        tx.execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS live_ids(id INTEGER PRIMARY KEY);
             DELETE FROM live_ids;",
        )?;
        {
            let mut stmt = tx.prepare("INSERT OR IGNORE INTO live_ids(id) VALUES(?1)")?;
            for id in live_ids {
                stmt.execute([id])?;
            }
        }
        let removed = tx.execute(
            "DELETE FROM roms WHERE id NOT IN (SELECT id FROM live_ids)",
            [],
        )?;
        tx.commit()?;
        Ok(removed)
    }

    /// Pull platforms and ROMs from the server into the cache.
    ///
    /// Returns `(platforms, roms_upserted, was_incremental)`.
    pub async fn sync(
        &mut self,
        client: &api::Client,
        force_full: bool,
    ) -> Result<(usize, usize, bool)> {
        let platforms = client.platforms().await?;
        {
            let tx = self.conn.transaction()?;
            for p in &platforms {
                // A slug can come back under a different id: deleting a platform
                // on the server and letting a scan recreate it renumbers it, and
                // the upsert below only resolves a conflict on `id`, leaving the
                // UNIQUE on `fs_slug` to fail. Drop the stale row first.
                //
                // Safe because `roms` keys off `platform_slug`, not this id, so
                // nothing downstream is orphaned by the renumbering.
                tx.execute(
                    "DELETE FROM platforms WHERE fs_slug = ?1 AND id <> ?2",
                    params![p.fs_slug, p.id],
                )?;
                tx.execute(
                    "INSERT INTO platforms(id, fs_slug, display_name, rom_count)
                     VALUES(?1, ?2, ?3, ?4)
                     ON CONFLICT(id) DO UPDATE SET
                        fs_slug = excluded.fs_slug,
                        display_name = excluded.display_name,
                        rom_count = excluded.rom_count",
                    params![
                        p.id,
                        p.fs_slug,
                        p.name.clone().unwrap_or_default(),
                        p.rom_count
                    ],
                )?;
            }
            tx.commit()?;
        }

        let since = if force_full { None } else { self.watermark() };
        let mut offset = 0u32;
        let mut upserted = 0usize;
        let mut high = since.clone().unwrap_or_default();

        loop {
            let page = client.roms(None, 500, offset, since.as_deref()).await?;
            if page.items.is_empty() {
                break;
            }
            let n = page.items.len();
            {
                let tx = self.conn.transaction()?;
                for rom in &page.items {
                    if let Some(ts) = &rom.updated_at
                        && ts.as_str() > high.as_str()
                    {
                        high = ts.clone();
                    }
                    tx.execute(
                        "INSERT INTO roms(id, platform_slug, name, fs_name,
                                          fs_size_bytes, md5_hash, sha1_hash,
                                          crc_hash, updated_at, cover_path,
                                          screenshot_path, screenshots_json,
                                          cover_small_path, summary, meta_json,
                                          alt_names_json, regions_json,
                                          manual_path, youtube_id, multi_file,
                                          last_played)
                         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,
                                ?14,?15,?16,?17,?18,?19,?20,?21)
                         ON CONFLICT(id) DO UPDATE SET
                            platform_slug = excluded.platform_slug,
                            name          = excluded.name,
                            fs_name       = excluded.fs_name,
                            fs_size_bytes = excluded.fs_size_bytes,
                            md5_hash      = excluded.md5_hash,
                            sha1_hash     = excluded.sha1_hash,
                            crc_hash      = excluded.crc_hash,
                            updated_at    = excluded.updated_at,
                            cover_path    = excluded.cover_path,
                            screenshot_path = excluded.screenshot_path,
                            screenshots_json = excluded.screenshots_json,
                            cover_small_path = excluded.cover_small_path,
                            summary        = excluded.summary,
                            meta_json      = excluded.meta_json,
                            alt_names_json = excluded.alt_names_json,
                            regions_json   = excluded.regions_json,
                            manual_path    = excluded.manual_path,
                            youtube_id     = excluded.youtube_id,
                            multi_file     = excluded.multi_file,
                            -- Only when the server has one. An incremental
                            -- pull can return a row with no per-user block,
                            -- and letting that null out the timestamp would
                            -- empty the recent list on every sync.
                            last_played    = COALESCE(excluded.last_played, roms.last_played)",
                        params![
                            rom.id,
                            rom.platform_fs_slug.clone().unwrap_or_default(),
                            rom.name.clone().unwrap_or_default(),
                            rom.fs_name,
                            rom.fs_size_bytes,
                            rom.md5_hash,
                            rom.sha1_hash,
                            rom.crc_hash,
                            rom.updated_at,
                            rom.path_cover_large,
                            rom.merged_screenshots.first(),
                            serde_json::to_string(&rom.merged_screenshots).ok(),
                            rom.path_cover_small,
                            rom.summary,
                            rom.metadatum.as_ref().and_then(|m| serde_json::to_string(m).ok()),
                            serde_json::to_string(&rom.alternative_names).ok(),
                            serde_json::to_string(&rom.regions).ok(),
                            rom.path_manual,
                            rom.youtube_video_id,
                            rom.has_multiple_files as i64,
                            rom.rom_user.as_ref().and_then(|u| u.last_played.clone()),
                        ],
                    )?;
                }
                tx.commit()?;
            }
            upserted += n;
            offset += n as u32;
            if page.total > 0 && offset as i64 >= page.total {
                break;
            }
        }

        if !high.is_empty() {
            self.meta_set("roms_updated_through", &high)?;
        }
        Ok((platforms.len(), upserted, since.is_some()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A cache on disk rather than in memory: `open` runs the migration path
    /// too, which is where the tolerant column reads below come from.
    fn cache(name: &str) -> Cache {
        let dir = std::env::temp_dir().join(format!("romm-cache-test-{name}"));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        Cache::open(&dir.join("c.sqlite3")).expect("opening a fresh cache")
    }

    fn add_platform(c: &Cache, id: i64, slug: &str, display: &str) {
        c.conn
            .execute(
                "INSERT INTO platforms(id, fs_slug, display_name, rom_count) VALUES(?1,?2,?3,0)",
                params![id, slug, display],
            )
            .unwrap();
    }

    /// Sessions, and what they add up to.
    ///
    /// The SQL here is the whole feature: a mistake in a JOIN or a GROUP BY
    /// does not fail, it produces a plausible number, and a plausible wrong
    /// number about how you spent a year is worse than no number.
    #[test]
    fn play_time_adds_up_per_game_and_per_console() {
        let c = cache("plays");
        add_platform(&c, 1, "snes", "Super Nintendo");
        add_platform(&c, 2, "psx", "PlayStation");
        add_rom(&c, 10, "snes", "Chrono Trigger", "ct.sfc");
        add_rom(&c, 11, "snes", "Super Metroid", "sm.sfc");
        add_rom(&c, 20, "psx", "Vagrant Story", "vs.bin");

        c.record_play(10, "2026-01-01T10:00:00", 3600).unwrap();
        c.record_play(10, "2026-01-02T10:00:00", 1800).unwrap();
        c.record_play(11, "2026-01-03T10:00:00", 600).unwrap();
        c.record_play(20, "2026-01-04T10:00:00", 7200).unwrap();

        let by_platform = c.play_by_platform().unwrap();
        // PlayStation first: two hours beats one and a half, and the ordering
        // is what the page is for.
        assert_eq!(by_platform[0].0, "psx");
        assert_eq!(by_platform[0].1, 7200);
        assert_eq!(by_platform[1], ("snes".to_owned(), 6000, 3, 2));

        let by_game = c.play_by_game(10).unwrap();
        assert_eq!(by_game[0].0.id, 20);
        assert_eq!(by_game[1].0.name, "Chrono Trigger");
        assert_eq!(by_game[1].1, 5400, "two sessions on one game must sum");
        assert_eq!(by_game[1].2, 2, "and count as two");
        assert_eq!(by_game[1].3, "2026-01-02T10:00:00", "the later of the two");

        assert_eq!(c.play_totals().unwrap(), (13_200, 4, 3));
    }

    /// Starting a game and quitting straight back out is something people do
    /// constantly — wrong game, wrong controller, checking it runs. Counting
    /// those makes a session count mean nothing.
    #[test]
    fn a_glance_at_a_game_is_not_a_session() {
        let c = cache("plays-short");
        add_platform(&c, 1, "snes", "Super Nintendo");
        add_rom(&c, 10, "snes", "Chrono Trigger", "ct.sfc");

        assert!(!c.record_play(10, "2026-01-01T10:00:00", 12).unwrap());
        assert!(!c.record_play(10, "2026-01-01T10:01:00", 59).unwrap());
        assert!(c.record_play(10, "2026-01-01T10:02:00", 60).unwrap());
        assert_eq!(c.play_totals().unwrap(), (60, 1, 1));
    }

    /// Playing something here has to show up on the "continue playing" row
    /// here. It used to wait on the server sending back a timestamp it had no
    /// reason to have, so the row showed what other machines had been doing.
    #[test]
    fn playing_a_game_marks_it_played_without_asking_the_server() {
        let c = cache("plays-recent");
        add_platform(&c, 1, "snes", "Super Nintendo");
        add_rom(&c, 10, "snes", "Chrono Trigger", "ct.sfc");
        assert!(c.recently_played(5).unwrap().is_empty());

        c.record_play(10, "2026-01-01T10:00:00", 900).unwrap();
        let recent = c.recently_played(5).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].id, 10);
    }

    /// "Started twice and bounced off" is a narrower question than "started
    /// once", which is just the library. A game played for an afternoon is not
    /// abandoned however many times it was opened, and one opened once is not
    /// yet a pattern.
    #[test]
    fn abandoned_means_came_back_to_and_still_bounced_off() {
        let c = cache("plays-abandoned");
        add_platform(&c, 1, "snes", "Super Nintendo");
        add_rom(&c, 10, "snes", "Bounced Off", "a.sfc");
        add_rom(&c, 11, "snes", "Played Properly", "b.sfc");
        add_rom(&c, 12, "snes", "Opened Once", "c.sfc");

        for i in 0..3 {
            c.record_play(10, &format!("2026-01-0{}T10:00:00", i + 1), 300).unwrap();
        }
        for i in 0..3 {
            c.record_play(11, &format!("2026-02-0{}T10:00:00", i + 1), 5000).unwrap();
        }
        c.record_play(12, "2026-03-01T10:00:00", 200).unwrap();

        let got = c.abandoned(2, 1800, 10).unwrap();
        let names: Vec<&str> = got.iter().map(|(r, _, _)| r.name.as_str()).collect();
        assert_eq!(names, ["Bounced Off"]);
        assert_eq!(got[0].1, 900);
        assert_eq!(got[0].2, 3);
    }

    fn add_rom(c: &Cache, id: i64, slug: &str, name: &str, fs_name: &str) {
        c.conn
            .execute(
                "INSERT INTO roms(id, platform_slug, name, fs_name, fs_size_bytes)
                 VALUES(?1,?2,?3,?4,0)",
                params![id, slug, name, fs_name],
            )
            .unwrap();
    }

    /// The grid must never advertise a platform that opens empty. The count
    /// comes from the roms actually held, not the server's figure, because the
    /// two disagree the moment anything is pruned.
    #[test]
    fn platforms_with_no_roms_are_not_offered() {
        let c = cache("empty-platforms");
        add_platform(&c, 1, "snes", "Super Nintendo");
        add_platform(&c, 2, "dc", "Dreamcast");
        add_rom(&c, 10, "snes", "Chrono Trigger", "ct.sfc");

        let got = c.platforms().unwrap();
        assert_eq!(got.len(), 1, "dreamcast holds nothing and must not appear");
        assert_eq!(got[0].fs_slug, "snes");
        assert_eq!(got[0].rom_count, 1);
    }

    /// Alphabetical by display name, case-insensitively. It was ordered by ROM
    /// count, which put the two biggest systems first and scattered the rest
    /// with no visible logic.
    #[test]
    fn platforms_are_ordered_by_name_regardless_of_case() {
        let c = cache("platform-order");
        for (i, (slug, display)) in
            [("z", "atari"), ("a", "Nintendo"), ("m", "Sega")].iter().enumerate()
        {
            add_platform(&c, i as i64 + 1, slug, display);
            add_rom(&c, i as i64 + 100, slug, "g", "g.bin");
        }
        let names: Vec<String> =
            c.platforms().unwrap().into_iter().map(|p| p.display_name).collect();
        assert_eq!(names, ["atari", "Nintendo", "Sega"], "lowercase must not sort last");
    }

    /// Incremental sync never learns about deletions, so pruning is the only
    /// thing that removes a stale row.
    #[test]
    fn pruning_drops_exactly_what_the_server_no_longer_has() {
        let mut c = cache("prune");
        add_rom(&c, 1, "snes", "Kept", "kept.sfc");
        add_rom(&c, 2, "snes", "Gone", "gone.sfc");
        add_rom(&c, 3, "snes", "Also gone", "gone2.sfc");

        assert_eq!(c.prune_missing(&[1]).unwrap(), 2);
        assert_eq!(c.rom_count().unwrap(), 1);
        assert!(c.rom_by_id(1).unwrap().is_some());
    }

    /// The guard that matters most: an empty id list means the server call
    /// failed, not that the server has nothing. Without this, one failed
    /// request would empty the entire library.
    #[test]
    fn pruning_against_an_empty_list_deletes_nothing() {
        let mut c = cache("prune-empty");
        add_rom(&c, 1, "snes", "Kept", "kept.sfc");
        assert_eq!(c.prune_missing(&[]).unwrap(), 0);
        assert_eq!(c.rom_count().unwrap(), 1, "an empty list must never wipe the cache");
    }

    /// Only bare romset names on arcade platforms are replaced. A real title is
    /// left alone, and a same-named file on another platform is not touched.
    #[test]
    fn arcade_renaming_only_touches_bare_romsets_on_arcade_platforms() {
        let mut c = cache("arcade-names");
        add_rom(&c, 1, "arcade", "kof98", "kof98.zip");
        add_rom(&c, 2, "arcade", "Metal Slug", "mslug.zip");
        add_rom(&c, 3, "snes", "kof98", "kof98.zip");

        let names = std::collections::BTreeMap::from([
            ("kof98".to_owned(), "The King of Fighters '98".to_owned()),
            ("mslug".to_owned(), "Metal Slug".to_owned()),
        ]);
        assert_eq!(c.apply_arcade_names(&names).unwrap(), 1);

        assert_eq!(c.rom_by_id(1).unwrap().unwrap().name, "The King of Fighters '98");
        assert_eq!(c.rom_by_id(2).unwrap().unwrap().name, "Metal Slug", "already correct");
        assert_eq!(c.rom_by_id(3).unwrap().unwrap().name, "kof98", "not an arcade platform");
    }

    /// An older cache stores this as TEXT because the migration adds columns
    /// loosely; a fresh one stores an integer. Reading it strictly would make
    /// every folder ROM in an existing cache look single-file, and download it
    /// as an unusable zip.
    #[test]
    fn multi_file_reads_from_both_the_old_and_new_column_types() {
        let c = cache("multifile");
        add_rom(&c, 1, "psx", "Int", "a.chd");
        add_rom(&c, 2, "psx", "Text", "b.chd");
        add_rom(&c, 3, "psx", "Zero", "c.chd");
        c.conn.execute("UPDATE roms SET multi_file = 1 WHERE id = 1", []).unwrap();
        c.conn.execute("UPDATE roms SET multi_file = '1' WHERE id = 2", []).unwrap();
        c.conn.execute("UPDATE roms SET multi_file = '0' WHERE id = 3", []).unwrap();

        assert!(c.rom_by_id(1).unwrap().unwrap().multi_file, "integer 1");
        assert!(c.rom_by_id(2).unwrap().unwrap().multi_file, "text \"1\"");
        assert!(!c.rom_by_id(3).unwrap().unwrap().multi_file, "text \"0\"");
    }

    /// A row with no display name falls back to its filename, or the UI shows
    /// a blank tile that cannot be identified or searched for.
    #[test]
    fn a_nameless_rom_falls_back_to_its_filename() {
        let c = cache("noname");
        add_rom(&c, 1, "snes", "", "Actraiser (USA).sfc");
        assert_eq!(c.rom_by_id(1).unwrap().unwrap().name, "Actraiser (USA).sfc");
    }

    /// Search covers the filename as well as the title, because half this
    /// library is known by one and half by the other.
    #[test]
    fn search_matches_title_or_filename_case_insensitively() {
        let c = cache("search");
        add_rom(&c, 1, "snes", "Chrono Trigger", "ct.sfc");
        add_rom(&c, 2, "arcade", "kof98", "kof98.zip");

        assert_eq!(c.search("chrono", 10).unwrap().len(), 1, "title, wrong case");
        assert_eq!(c.search("ct.sfc", 10).unwrap().len(), 1, "filename");
        assert_eq!(c.search("KOF", 10).unwrap().len(), 1);
        assert_eq!(c.search("nothing here", 10).unwrap().len(), 0);
    }

    /// The newest schema stores every screenshot; older caches stored one. A
    /// cache written before the list existed must still show its screenshot.
    #[test]
    fn screenshots_prefer_the_list_and_fall_back_to_the_single_column() {
        let c = cache("shots");
        add_rom(&c, 1, "snes", "Both", "a.sfc");
        add_rom(&c, 2, "snes", "Legacy", "b.sfc");
        c.conn
            .execute(
                "UPDATE roms SET screenshots_json = '[\"/one.png\",\"/two.png\"]',
                                 screenshot_path = '/old.png' WHERE id = 1",
                [],
            )
            .unwrap();
        c.conn
            .execute("UPDATE roms SET screenshot_path = '/old.png' WHERE id = 2", [])
            .unwrap();

        assert_eq!(
            c.rom_by_id(1).unwrap().unwrap().screenshots(),
            ["/one.png", "/two.png"]
        );
        assert_eq!(c.rom_by_id(2).unwrap().unwrap().screenshots(), ["/old.png"]);
    }

    /// An empty stored list must not shadow the legacy column, or a row that
    /// synced before the list existed shows no artwork at all.
    #[test]
    fn an_empty_screenshot_list_falls_back_rather_than_showing_nothing() {
        let c = cache("shots-empty");
        add_rom(&c, 1, "snes", "Empty list", "a.sfc");
        c.conn
            .execute(
                "UPDATE roms SET screenshots_json = '[]', screenshot_path = '/old.png'
                 WHERE id = 1",
                [],
            )
            .unwrap();
        assert_eq!(c.rom_by_id(1).unwrap().unwrap().screenshots(), ["/old.png"]);
    }

    /// Collections come from the server as JSON, and the two families disagree
    /// on the type of `id` — hand-made ones use a number, virtual ones a base64
    /// string. Both have to land in the same table.
    #[test]
    fn collections_accept_both_numeric_and_string_ids() {
        let numeric: crate::api::Collection =
            serde_json::from_str(r#"{"id": 5, "name": "Favourites", "rom_ids": [1]}"#).unwrap();
        assert_eq!(numeric.id, "5");
        assert_eq!(numeric.group(), "user");

        let virt: crate::api::Collection = serde_json::from_str(
            r#"{"id": "eyJuYW1lIjoiUlBHIn0", "name": "RPG", "is_virtual": true,
                "type": "genre", "rom_ids": [1]}"#,
        )
        .unwrap();
        assert_eq!(virt.id, "eyJuYW1lIjoiUlBHIn0");
        assert_eq!(virt.group(), "genre", "a virtual collection groups by its type");
    }

    /// A collection whose members are all gone would open empty, so it is not
    /// offered — and neither is a group left with nothing in it.
    #[test]
    fn collections_that_would_open_empty_are_hidden() {
        let mut c = cache("collections");
        add_rom(&c, 1, "snes", "Chrono Trigger", "ct.sfc");

        let live: crate::api::Collection =
            serde_json::from_str(r#"{"id": 1, "name": "Live", "rom_ids": [1]}"#).unwrap();
        // Every member of this one was pruned from the cache.
        let dead: crate::api::Collection =
            serde_json::from_str(r#"{"id": 2, "name": "Dead", "rom_ids": [999]}"#).unwrap();
        c.replace_collections(&[live, dead]).unwrap();

        let groups = c.collection_groups().unwrap();
        assert_eq!(groups, [("user".to_owned(), 1)], "only the collection with a live member");

        let items = c.collections_in("user").unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].name, "Live");
        assert_eq!(items[0].sample_ids, [1], "sample ids drive the cover mosaic");
    }

    /// Replacing is wholesale: virtual collection ids are derived from name and
    /// type, so a rename orphans the old row rather than updating it.
    #[test]
    fn replacing_collections_clears_the_previous_set() {
        let mut c = cache("collections-replace");
        add_rom(&c, 1, "snes", "Game", "g.sfc");

        let first: crate::api::Collection =
            serde_json::from_str(r#"{"id": 1, "name": "Old name", "rom_ids": [1]}"#).unwrap();
        c.replace_collections(&[first]).unwrap();
        let renamed: crate::api::Collection =
            serde_json::from_str(r#"{"id": 2, "name": "New name", "rom_ids": [1]}"#).unwrap();
        c.replace_collections(&[renamed]).unwrap();

        let items = c.collections_in("user").unwrap();
        assert_eq!(items.len(), 1, "the orphaned row must be gone, not accumulated");
        assert_eq!(items[0].name, "New name");
    }

    /// An ES-DE library lives wherever the user put it, so a game's location
    /// cannot be derived from <roms>/<slug>/<file>. The index is asked instead.
    #[test]
    fn a_scanned_games_platform_is_found_by_its_absolute_path() {
        let c = cache("path-lookup");
        add_rom(&c, 1, "genesis", "Sonic", "sonic.md");
        c.conn
            .execute(
                "UPDATE roms SET local_path = '/Volumes/SD/ROMs/megadrive/sonic.md' WHERE id = 1",
                [],
            )
            .unwrap();

        assert_eq!(
            c.platform_for_path(Path::new("/Volumes/SD/ROMs/megadrive/sonic.md")).as_deref(),
            Some("genesis")
        );
        assert_eq!(c.platform_for_path(Path::new("/nowhere/sonic.md")), None);
    }

    /// The exclusion lists govern archive hashing, so they must survive a
    /// restart with the server unreachable — otherwise an offline verify uses
    /// different rules from the download that produced the file.
    #[test]
    fn server_exclusions_round_trip_for_offline_use() {
        let c = cache("server-config");
        assert!(c.server_exclusions().is_none(), "nothing known before the first sync");

        c.save_server_config(&crate::api::ServerConfig {
            default_excluded_files: vec!["custom.nfo".to_owned()],
            default_excluded_extensions: vec!["sav".to_owned()],
            skip_hash_calculation: false,
        })
        .unwrap();

        let (files, exts) = c.server_exclusions().expect("stored");
        assert_eq!(files, ["custom.nfo"]);
        assert_eq!(exts, ["sav"]);
    }

    /// The row of recent games is only useful if it survives a sync. An
    /// incremental pull can return a game with no per-user block at all, and
    /// letting that overwrite the timestamp empties the list every time.
    #[test]
    fn a_sync_without_per_user_data_does_not_forget_when_a_game_was_played() {
        let c = cache("last-played");
        add_platform(&c, 1, "snes", "Super Nintendo");
        add_rom(&c, 10, "snes", "Chrono Trigger", "ct.sfc");
        c.conn
            .execute("UPDATE roms SET last_played = '2026-08-01T10:00:00' WHERE id = 10", [])
            .unwrap();
        assert_eq!(c.recently_played(5).unwrap().len(), 1);

        // What an incremental sync does when the server sends no rom_user.
        c.conn
            .execute(
                "INSERT INTO roms(id, platform_slug, name, fs_name, fs_size_bytes, last_played)
                 VALUES(10,'snes','Chrono Trigger','ct.sfc',0,NULL)
                 ON CONFLICT(id) DO UPDATE SET
                    last_played = COALESCE(excluded.last_played, roms.last_played)",
                [],
            )
            .unwrap();
        assert_eq!(
            c.recently_played(5).unwrap().len(),
            1,
            "the timestamp must survive a sync that did not mention it"
        );
    }

    #[test]
    fn recent_games_come_back_newest_first_and_never_the_unplayed() {
        let c = cache("recent-order");
        add_platform(&c, 1, "snes", "Super Nintendo");
        for (id, name, when) in [
            (1, "Older", Some("2026-01-01T00:00:00")),
            (2, "Newer", Some("2026-08-01T00:00:00")),
            (3, "Never", None),
        ] {
            add_rom(&c, id, "snes", name, &format!("{name}.sfc"));
            if let Some(w) = when {
                c.conn
                    .execute("UPDATE roms SET last_played = ?1 WHERE id = ?2", rusqlite::params![w, id])
                    .unwrap();
            }
        }
        let got = c.recently_played(10).unwrap();
        assert_eq!(got.len(), 2, "a game never played has no place in a recent list");
        assert_eq!(got[0].name, "Newer");
        assert_eq!(got[1].name, "Older");
    }

    #[test]
    fn the_recent_list_honours_its_limit() {
        let c = cache("recent-limit");
        add_platform(&c, 1, "snes", "Super Nintendo");
        for i in 1..=8 {
            add_rom(&c, i, "snes", &format!("Game {i}"), &format!("g{i}.sfc"));
            c.conn
                .execute(
                    "UPDATE roms SET last_played = ?1 WHERE id = ?2",
                    rusqlite::params![format!("2026-08-{:02}T00:00:00", i), i],
                )
                .unwrap();
        }
        assert_eq!(c.recently_played(3).unwrap().len(), 3);
    }
}
