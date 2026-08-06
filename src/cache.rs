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

#[derive(Debug, Clone)]
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

    pub fn roms_for(&self, platform_slug: &str) -> Result<Vec<RomRow>> {
        let sql = format!(
            "SELECT {ROM_COLUMNS} FROM roms WHERE platform_slug = ?1 \
             ORDER BY 3 COLLATE NOCASE"
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
                                          manual_path, youtube_id, multi_file)
                         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,
                                ?14,?15,?16,?17,?18,?19,?20)
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
                            multi_file     = excluded.multi_file",
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
