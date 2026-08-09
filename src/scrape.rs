//! Filling in artwork for games ES-DE never scraped.
//!
//! ## Why this does not talk to ScreenScraper itself
//!
//! ScreenScraper rejects every request that does not carry a *developer*
//! credential, and those are issued to registered applications, not to people.
//! An account of your own is not enough. So a client like this one has three
//! ways to get art, and only one of them is honest:
//!
//! * Take another application's credential — ES-DE's, say, or RomM's — out of
//!   its binary or its URLs and make our own calls under its name. That is
//!   impersonating someone else's software to a service that trusted it, and
//!   when it is noticed the key is revoked for *their* users, who did nothing.
//!   Not an option, however convenient.
//! * Register this app with ScreenScraper and wait for its own credential.
//!   Correct, and slow.
//! * Ask the server, which already has one.
//!
//! The third is what happens here, and it needs nothing new. A RomM server with
//! ScreenScraper configured answers `/api/search/roms` with ready-made media
//! URLs — the same call and the same URLs its own web interface uses when it
//! offers you match candidates. Fetching one is being an ordinary client of
//! your own server. Nothing is borrowed and nothing is pretended.
//!
//! The cost of staying inside that line: the server builds one URL per game,
//! for the 2D box. Cartridge art, miximages and the rest would mean assembling
//! ScreenScraper calls the server never made, under its name, which is the
//! first option wearing a different hat.
//!
//! ## Where the art goes
//!
//! Into the same local cache the ES-DE tree is copied to, filed under `covers`,
//! because a 2D box is what it is. So it lands as the third link of the art
//! chain: a console showing cartridges keeps showing cartridges, and only the
//! games that had nothing gain a box.

use anyhow::Result;
use std::path::Path;

use crate::api::Client;
use crate::cache::{Cache, RomRow};
use crate::media;

/// What a run did.
#[derive(Debug, Default, Clone, Copy)]
pub struct Report {
    /// Games that already had artwork and were not touched.
    pub had_art: usize,
    pub fetched: usize,
    /// Identified by the server, but ScreenScraper had no picture.
    pub no_art: usize,
    /// The server could not identify the file at all.
    pub unmatched: usize,
    pub failed: usize,
}

impl Report {
    pub fn describe(&self) -> String {
        format!(
            "{} fetched, {} already had art, {} not on ScreenScraper, {} unidentified{}",
            self.fetched,
            self.had_art,
            self.no_art,
            self.unmatched,
            if self.failed > 0 { format!(", {} failed", self.failed) } else { String::new() },
        )
    }
}

/// Games with no artwork of any kind, in the order they would be shown.
///
/// Checked against the local cache only, which is deliberate: a HEAD per game
/// per media type against the server would be seven requests each before any
/// work started, and the caller is about to ask the server about every one of
/// them anyway.
pub fn missing(cache: &Cache, media_root: &Path, platform: Option<&str>) -> Result<Vec<RomRow>> {
    let rows = match platform {
        Some(p) => cache.roms_for(p)?,
        None => cache.all_roms()?,
    };
    Ok(rows
        .into_iter()
        .filter(|r| {
            let stem = stem_of(&r.fs_name);
            !media::ART_CHAIN
                .iter()
                .any(|k| media::find_local(media_root, &r.platform_slug, &stem, k).is_some())
        })
        .collect())
}

/// Fetch artwork for one game. Returns whether anything landed.
///
/// Every step is allowed to come back empty. A romset with no ScreenScraper
/// entry, a game the server cannot identify, a match with no picture attached —
/// all ordinary, and none of them a reason to stop a run of two thousand.
pub async fn fill_one(
    client: &Client,
    media_root: &Path,
    row: &RomRow,
    report: &mut Report,
) -> Result<bool> {
    let stem = stem_of(&row.fs_name);

    let matches = match client.identify(row.id, &row.name).await {
        Ok(m) => m,
        Err(_) => {
            report.failed += 1;
            return Ok(false);
        }
    };
    let Some(hit) = matches.iter().find(|m| !m.ss_url_cover.is_empty()) else {
        if matches.is_empty() {
            report.unmatched += 1;
        } else {
            report.no_art += 1;
        }
        return Ok(false);
    };

    // Straight to ScreenScraper, at the address the server gave us, with no
    // Authorization header — that is our server's credential, not theirs.
    let resp = client.http().get(&hit.ss_url_cover).send().await;
    let bytes = match resp {
        Ok(r) if r.status().is_success() => r.bytes().await.unwrap_or_default(),
        _ => {
            report.failed += 1;
            return Ok(false);
        }
    };
    // ScreenScraper answers a throttled or unknown request with a short plain
    // text body and a 200, so size is the honest test of whether this is a
    // picture. Every real cover is tens of kilobytes.
    if bytes.len() < 1024 {
        report.no_art += 1;
        return Ok(false);
    }

    let dir = media_root.join(&row.platform_slug).join(media::COVERS);
    std::fs::create_dir_all(&dir)?;
    let ext = if bytes.starts_with(b"\x89PNG") { "png" } else { "jpg" };
    std::fs::write(dir.join(format!("{stem}.{ext}")), &bytes)?;
    report.fetched += 1;
    Ok(true)
}

fn stem_of(fs_name: &str) -> String {
    Path::new(fs_name)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| fs_name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_that_found_nothing_still_says_what_it_looked_at() {
        let r = Report { had_art: 12, ..Default::default() };
        assert!(r.describe().contains("0 fetched"));
        assert!(r.describe().contains("12 already had art"));
        // Failures are only mentioned when there were some: a clean run should
        // not read as though something went wrong.
        assert!(!r.describe().contains("failed"));
        assert!(Report { failed: 3, ..Default::default() }.describe().contains("3 failed"));
    }

    #[test]
    fn a_name_with_no_extension_still_yields_a_stem() {
        assert_eq!(stem_of("Sonic (USA).md"), "Sonic (USA)");
        assert_eq!(stem_of("pkscram"), "pkscram");
        assert_eq!(stem_of("Game.v1.2.zip"), "Game.v1.2");
    }
}
