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

/// Every ES-DE media type, with the extensions each realistically uses.
///
/// The server exposes these at
/// `/assets/romm/resources/esde-media/<platform>/<type>/<rom basename>.<ext>`
/// — symlinks into the ROM library from RomM's own resources tree, which is a
/// mounted volume and already web-served. Far richer than RomM's own metadata,
/// which carries one cover and one screenshot.
pub const ESDE_TYPES: &[(&str, &[&str])] = &[
    ("covers", &["png", "jpg", "webp"]),
    ("backcovers", &["png", "jpg", "webp"]),
    ("3dboxes", &["png", "jpg", "webp"]),
    ("miximages", &["png", "jpg", "webp"]),
    ("screenshots", &["png", "jpg", "webp"]),
    ("titlescreens", &["png", "jpg", "webp"]),
    ("marquees", &["png", "jpg", "webp"]),
    ("fanart", &["png", "jpg", "webp"]),
    ("physicalmedia", &["png", "jpg", "webp"]),
    ("videos", &["mp4", "webm", "mkv"]),
    ("manuals", &["pdf"]),
];

/// Path prefix the media symlinks live under, relative to the server root.
const ESDE_BASE: &str = "/assets/romm/resources/esde-media";

/// ES-DE media subdirectory names, in the order the UI prefers them.
pub const COVERS: &str = "covers";
pub const SCREENSHOTS: &str = "screenshots";
pub const VIDEOS: &str = "videos";
/// Thumbnails for the grid. Not an ES-DE directory — ES-DE has no thumb
/// concept — but kept in the same tree so one delete clears everything.
pub const COVERS_THUMB: &str = "covers_thumb";
pub const MANUALS: &str = "manuals";

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

/// Fetch one ES-DE media file from the server, trying the extensions that
/// type actually uses.
///
/// This path needs no authentication (RomM serves `/assets` openly), so it
/// works even before the client has credentials.
pub async fn ensure_esde(
    client: Option<&api::Client>,
    media_root: &Path,
    platform: &str,
    stem: &str,
    kind: &str,
) -> Option<PathBuf> {
    if let Some(local) = find_local(media_root, platform, stem, kind) {
        return Some(local);
    }
    let client = client?;
    let exts = ESDE_TYPES.iter().find(|(k, _)| *k == kind).map(|(_, e)| *e)?;

    for ext in exts {
        let server_path = format!("{ESDE_BASE}/{platform}/{kind}/{stem}.{ext}");
        match fetch(client, &server_path, media_root, platform, stem, kind).await {
            Ok(p) => return Some(p),
            // A miss here is ordinary: not every game has every media type,
            // and we are probing extensions.
            Err(_) => continue,
        }
    }
    None
}

/// Grid thumbnail: the small cover if the server has one, otherwise whatever
/// full-size cover is already local (an imported ES-DE one, typically).
pub async fn ensure_thumb(
    client: Option<&api::Client>,
    media_root: &Path,
    platform: &str,
    stem: &str,
    small: Option<&str>,
    large: Option<&str>,
) -> Option<PathBuf> {
    if let Some(p) = find_local(media_root, platform, stem, COVERS_THUMB) {
        return Some(p);
    }
    // An already-downloaded full cover beats spending a request on a thumb.
    if let Some(p) = find_local(media_root, platform, stem, COVERS) {
        return Some(p);
    }
    if small.is_some() {
        if let Some(p) = ensure(client, media_root, platform, stem, COVERS_THUMB, small).await {
            return Some(p);
        }
    }
    ensure(client, media_root, platform, stem, COVERS, large).await
}

/// Resolve a set of screenshots: any already local, plus anything the server
/// has that we do not.
///
/// Extras are stored as `<stem>-2.jpg`, `<stem>-3.jpg` … because the ES-DE
/// convention only names one screenshot per game, and we do not want to
/// collide with an imported one.
pub async fn ensure_set(
    client: Option<&api::Client>,
    media_root: &Path,
    platform: &str,
    stem: &str,
    server_paths: &[String],
) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for (i, server) in server_paths.iter().enumerate() {
        let name = if i == 0 {
            stem.to_owned()
        } else {
            format!("{stem}-{}", i + 1)
        };
        if let Some(p) = ensure(
            client,
            media_root,
            platform,
            &name,
            SCREENSHOTS,
            Some(server.as_str()),
        )
        .await
        {
            out.push(p);
        }
    }
    // No server list at all (or nothing fetched): fall back to whatever is on
    // disk under the plain stem.
    if out.is_empty()
        && let Some(p) = find_local(media_root, platform, stem, SCREENSHOTS)
    {
        out.push(p);
    }
    out
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


/// Pixel dimensions of a PNG or JPEG, without decoding the image.
///
/// Only the header is read: PNG keeps them in IHDR, JPEG in the SOFn marker.
/// Enough to work out a cover's aspect ratio cheaply.
pub fn image_size(path: &Path) -> Option<(u32, u32)> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let mut head = [0u8; 8];
    f.read_exact(&mut head).ok()?;

    if head == [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A] {
        let mut ihdr = [0u8; 16];
        f.read_exact(&mut ihdr).ok()?;
        let w = u32::from_be_bytes(ihdr[8..12].try_into().ok()?);
        let h = u32::from_be_bytes(ihdr[12..16].try_into().ok()?);
        return (w > 0 && h > 0).then_some((w, h));
    }

    if head[0] == 0xFF && head[1] == 0xD8 {
        f.seek(SeekFrom::Start(2)).ok()?;
        let mut b = [0u8; 1];
        loop {
            // Scan to the next marker.
            while f.read_exact(&mut b).is_ok() && b[0] != 0xFF {}
            let mut marker = 0xFFu8;
            while marker == 0xFF {
                f.read_exact(&mut b).ok()?;
                marker = b[0];
            }
            // SOF0..SOF15, skipping the non-frame markers in that range.
            if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
                let mut sof = [0u8; 7];
                f.read_exact(&mut sof).ok()?;
                let h = u16::from_be_bytes([sof[3], sof[4]]) as u32;
                let w = u16::from_be_bytes([sof[5], sof[6]]) as u32;
                return (w > 0 && h > 0).then_some((w, h));
            }
            let mut len = [0u8; 2];
            f.read_exact(&mut len).ok()?;
            let skip = u16::from_be_bytes(len).saturating_sub(2) as i64;
            f.seek(SeekFrom::Current(skip)).ok()?;
        }
    }
    None
}

/// Typical cover aspect (width / height) for a platform, from the covers we
/// already hold.
///
/// Box art varies enormously by system — measured here, PSP UMD cases are 0.58
/// and SNES boxes 1.37 — so a single grid ratio crops most of them. Sampling
/// real files keeps this correct without a hardcoded table to maintain.
///
/// Returns `None` until enough covers are cached to be confident.
pub fn cover_aspect(media_root: &Path, platform: &str) -> Option<f32> {
    let mut ratios: Vec<f32> = Vec::new();
    for kind in [COVERS_THUMB, COVERS] {
        let dir = media_root.join(platform).join(kind);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten().take(40) {
            if let Some((w, h)) = image_size(&entry.path())
                && h > 0
            {
                ratios.push(w as f32 / h as f32);
            }
        }
        if ratios.len() >= 8 {
            break;
        }
    }
    if ratios.len() < 3 {
        return None;
    }
    // Median: a few odd covers (a wide promo shot among box art) should not
    // drag the whole grid.
    ratios.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Some(ratios[ratios.len() / 2])
}
