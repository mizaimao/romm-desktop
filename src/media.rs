//! Artwork resolution: local ES-DE media first, server as fallback.
//!
//! Only ~2% of this library has local ES-DE media (it was staged for a 239-game
//! test subset), while the server has covers for most ROMs. Anything fetched is
//! written into the *same* ES-DE-shaped tree, so there is one lookup path and
//! imported and fetched art are interchangeable:
//!
//! ```text
//! <media_root>/<platform>/covers/<rom basename>.<ext>
//! <media_root>/<platform>/screenshots/<rom basename>.<ext>
//! <media_root>/<platform>/videos/<rom basename>.mp4
//! ```

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::api;

/// ES-DE media subdirectory names, in the order the UI prefers them.
pub const COVERS: &str = "covers";
pub const SCREENSHOTS: &str = "screenshots";
pub const VIDEOS: &str = "videos";

/// Look for an already-present media file, whatever its extension.
pub fn find_local(media_root: &Path, platform: &str, stem: &str, kind: &str) -> Option<PathBuf> {
    let dir = media_root.join(platform).join(kind);
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.file_stem().is_some_and(|s| s == stem) && path.is_file() {
            return path.canonicalize().ok();
        }
    }
    None
}

/// Percent-encode a server path, preserving `/` and `?`.
///
/// RomM emits cover URLs like `.../big.png?ts=2026-07-30 00:45:10` — the raw
/// space in the timestamp makes the request line invalid, so it must be
/// encoded before use.
fn encode_path(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() + 8);
    for b in raw.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' => out.push(b as char),
            b'-' | b'_' | b'.' | b'~' | b'/' | b'?' | b'=' | b'&' | b':' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn extension_of(server_path: &str) -> &str {
    let no_query = server_path.split('?').next().unwrap_or(server_path);
    match no_query.rsplit_once('.') {
        Some((_, ext)) if ext.len() <= 5 && ext.chars().all(|c| c.is_ascii_alphanumeric()) => ext,
        _ => "png",
    }
}

/// Fetch one artwork file from the server and store it in the media tree.
pub async fn fetch(
    client: &api::Client,
    server_path: &str,
    media_root: &Path,
    platform: &str,
    stem: &str,
    kind: &str,
) -> Result<PathBuf> {
    let url = format!(
        "{}/{}",
        client.base().trim_end_matches('/'),
        encode_path(server_path.trim_start_matches('/'))
    );

    let resp = client
        .http()
        .get(&url)
        .header("Authorization", format!("Basic {}", client.auth()))
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;

    if !resp.status().is_success() {
        bail!("GET {url} -> {}", resp.status());
    }
    let bytes = resp.bytes().await.context("reading artwork body")?;
    if bytes.is_empty() {
        bail!("empty artwork response from {url}");
    }

    let dir = media_root.join(platform).join(kind);
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let path = dir.join(format!("{stem}.{}", extension_of(server_path)));
    std::fs::write(&path, &bytes).with_context(|| format!("writing {}", path.display()))?;
    path.canonicalize().or(Ok(path))
}

/// Local file if present, else fetch from the server and cache it.
///
/// Returns `None` when there is nothing local *and* the server has no artwork
/// for this ROM — a normal outcome, not an error.
pub async fn ensure(
    client: Option<&api::Client>,
    media_root: &Path,
    platform: &str,
    stem: &str,
    kind: &str,
    server_path: Option<&str>,
) -> Option<PathBuf> {
    if let Some(local) = find_local(media_root, platform, stem, kind) {
        return Some(local);
    }
    let (client, server_path) = (client?, server_path?);
    if server_path.is_empty() {
        return None;
    }
    match fetch(client, server_path, media_root, platform, stem, kind).await {
        Ok(p) => Some(p),
        Err(e) => {
            eprintln!("artwork fetch failed for {platform}/{stem} ({kind}): {e}");
            None
        }
    }
}
