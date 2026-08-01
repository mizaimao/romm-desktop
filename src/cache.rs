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
    updated_at     TEXT
);
CREATE INDEX IF NOT EXISTS roms_platform ON roms(platform_slug);
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
"#;

pub struct Cache {
    conn: Connection,
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
}

/// Columns every `RomRow` query selects, in order.
const ROM_COLUMNS: &str = "id, platform_slug, COALESCE(NULLIF(name, ''), fs_name), \
                           fs_name, COALESCE(fs_size_bytes, 0), md5_hash, sha1_hash";

fn rom_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<RomRow> {
    Ok(RomRow {
        id: r.get(0)?,
        platform_slug: r.get(1)?,
        name: r.get(2)?,
        fs_name: r.get(3)?,
        fs_size_bytes: r.get(4)?,
        md5_hash: r.get(5)?,
        sha1_hash: r.get(6)?,
    })
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
        Ok(Self { conn })
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
             ORDER BY 3 DESC, 1 ASC",
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
                                          crc_hash, updated_at)
                         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)
                         ON CONFLICT(id) DO UPDATE SET
                            platform_slug = excluded.platform_slug,
                            name          = excluded.name,
                            fs_name       = excluded.fs_name,
                            fs_size_bytes = excluded.fs_size_bytes,
                            md5_hash      = excluded.md5_hash,
                            sha1_hash     = excluded.sha1_hash,
                            crc_hash      = excluded.crc_hash,
                            updated_at    = excluded.updated_at",
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
