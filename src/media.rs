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
        .header("Authorization", client.auth())
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
    if small.is_some()
        && let Some(p) = ensure(client, media_root, platform, stem, COVERS_THUMB, small).await {
            return Some(p);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("romm-media-test-{name}"));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A PNG header only: signature, then the IHDR chunk whose first two fields
    /// are the dimensions. Enough for `image_size`, which never decodes pixels.
    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut v = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        v.extend_from_slice(&13u32.to_be_bytes());
        v.extend_from_slice(b"IHDR");
        v.extend_from_slice(&w.to_be_bytes());
        v.extend_from_slice(&h.to_be_bytes());
        v
    }

    /// A JPEG reduced to SOI followed by an SOF0 frame header, which is where
    /// the dimensions live — height first, then width.
    fn jpeg(w: u16, h: u16) -> Vec<u8> {
        let mut v = vec![0xFF, 0xD8, 0xFF, 0xC0, 0x00, 0x11, 0x08];
        v.extend_from_slice(&h.to_be_bytes());
        v.extend_from_slice(&w.to_be_bytes());
        v
    }

    /// RomM emits cover URLs carrying a timestamp with a raw space in it, which
    /// makes the HTTP request line invalid. `/` and `?` have to survive, or the
    /// path stops addressing anything.
    #[test]
    fn a_server_path_is_encoded_without_destroying_its_structure() {
        let raw = "/assets/romm/resources/roms/2/42/cover/big.png?ts=2026-07-30 00:45:10";
        let got = encode_path(raw);
        assert!(!got.contains(' '), "the raw space must be gone: {got}");
        assert!(got.contains("%20"));
        assert!(got.starts_with("/assets/romm/"), "slashes are preserved: {got}");
        assert!(got.contains("?ts="), "the query still addresses the same asset: {got}");
        assert!(got.contains("00%3A45%3A10") || got.contains("00:45:10"));
    }

    /// The stored file's extension comes from the server path, and the query
    /// string is not part of it — `big.png?ts=…` must not become `png?ts=…`.
    #[test]
    fn the_extension_ignores_the_query_string() {
        assert_eq!(extension_of("/x/big.png?ts=2026-07-30 00:45:10"), "png");
        assert_eq!(extension_of("/x/cover.jpg"), "jpg");
        assert_eq!(extension_of("/x/clip.webm"), "webm");
    }

    /// Anything that does not look like an extension falls back to png rather
    /// than producing a file named after half a URL.
    #[test]
    fn an_unusable_extension_falls_back_to_png() {
        assert_eq!(extension_of("/x/no-extension-here"), "png");
        // Too long to be an extension — this is a path segment with a dot.
        assert_eq!(extension_of("/x/file.somethinglong"), "png");
        // Not alphanumeric.
        assert_eq!(extension_of("/x/file.p n g"), "png");
    }

    /// Imported ES-DE art and art fetched from the server share one tree, so
    /// the lookup must match on stem regardless of which extension landed.
    #[test]
    fn local_media_is_found_by_stem_whatever_the_extension() {
        let root = scratch("find-local");
        let dir = root.join("snes").join(COVERS);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Zelda.webp"), b"img").unwrap();

        assert!(find_local(&root, "snes", "Zelda", COVERS).is_some());
        // A different game, a different platform and a different media type
        // must all miss rather than returning the wrong file.
        assert!(find_local(&root, "snes", "Zeld", COVERS).is_none());
        assert!(find_local(&root, "nes", "Zelda", COVERS).is_none());
        assert!(find_local(&root, "snes", "Zelda", VIDEOS).is_none());
    }

    /// Dimensions are read from the header alone. Getting the PNG field order
    /// wrong transposes every cover; in JPEG the two are stored height-first,
    /// which is the easy mistake.
    #[test]
    fn image_dimensions_are_read_from_the_header_of_both_formats() {
        let dir = scratch("image-size");
        let p = dir.join("a.png");
        std::fs::write(&p, png(600, 800)).unwrap();
        assert_eq!(image_size(&p), Some((600, 800)));

        let j = dir.join("b.jpg");
        std::fs::write(&j, jpeg(320, 240)).unwrap();
        assert_eq!(image_size(&j), Some((320, 240)), "JPEG stores height before width");
    }

    /// Anything that is not a readable image must decline rather than return a
    /// nonsense ratio that reshapes the whole grid.
    #[test]
    fn a_file_that_is_not_an_image_has_no_size() {
        let dir = scratch("image-size-bad");
        let txt = dir.join("notes.txt");
        std::fs::write(&txt, b"not an image at all").unwrap();
        assert_eq!(image_size(&txt), None);
        assert_eq!(image_size(&dir.join("missing.png")), None);

        // A PNG claiming zero height would divide by zero downstream.
        let zero = dir.join("zero.png");
        std::fs::write(&zero, png(100, 0)).unwrap();
        assert_eq!(image_size(&zero), None);
    }

    /// Box art varies enormously by system, so the grid measures rather than
    /// assumes. The median is used so one odd wide promo shot cannot reshape
    /// every card on the page.
    #[test]
    fn the_cover_aspect_is_the_median_and_ignores_an_outlier() {
        let root = scratch("aspect");
        let dir = root.join("snes").join(COVERS);
        std::fs::create_dir_all(&dir).unwrap();
        // Four portrait covers at 0.5, and one very wide outlier.
        for i in 0..4 {
            std::fs::write(dir.join(format!("box{i}.png")), png(500, 1000)).unwrap();
        }
        std::fs::write(dir.join("promo.png"), png(4000, 1000)).unwrap();

        let aspect = cover_aspect(&root, "snes").expect("five covers is enough");
        assert!(
            (aspect - 0.5).abs() < 0.01,
            "the median must ignore the 4.0 outlier, got {aspect}"
        );
    }

    /// Too few covers means no confident answer, and the grid keeps its default
    /// rather than shaping itself around one or two files.
    #[test]
    fn too_few_covers_yields_no_aspect_at_all() {
        let root = scratch("aspect-few");
        let dir = root.join("snes").join(COVERS);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(cover_aspect(&root, "snes"), None, "nothing cached yet");

        std::fs::write(dir.join("a.png"), png(500, 1000)).unwrap();
        std::fs::write(dir.join("b.png"), png(500, 1000)).unwrap();
        assert_eq!(cover_aspect(&root, "snes"), None, "two is not enough to be confident");

        std::fs::write(dir.join("c.png"), png(500, 1000)).unwrap();
        assert!(cover_aspect(&root, "snes").is_some(), "three is");
    }

    /// Every declared media type needs at least one extension to probe, or
    /// `ensure_esde` silently never fetches it.
    #[test]
    fn every_media_type_has_extensions_to_try() {
        for (kind, exts) in ESDE_TYPES {
            assert!(!exts.is_empty(), "{kind} has no extensions and could never be fetched");
        }
        // The named constants must all be types that actually exist in the
        // table, or a lookup by that name finds nothing.
        for kind in [COVERS, SCREENSHOTS, VIDEOS, MANUALS] {
            assert!(
                ESDE_TYPES.iter().any(|(k, _)| *k == kind),
                "{kind} is referenced by name but not declared"
            );
        }
    }

    /// With no client there is nothing to fetch from, so these must resolve
    /// from disk alone rather than erroring — this is the offline path.
    #[tokio::test]
    async fn artwork_resolves_offline_from_local_files_only() {
        let root = scratch("offline");
        let dir = root.join("snes").join(COVERS);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Zelda.png"), b"img").unwrap();

        assert!(ensure(None, &root, "snes", "Zelda", COVERS, None).await.is_some());
        assert!(ensure(None, &root, "snes", "Missing", COVERS, None).await.is_none());
        // A local full cover is preferred over spending a request on a thumb.
        assert!(ensure_thumb(None, &root, "snes", "Zelda", None, None).await.is_some());
        assert!(ensure_esde(None, &root, "snes", "Zelda", COVERS).await.is_some());
    }

    /// Extra screenshots are stored as `<stem>-2`, `<stem>-3` so a fetched set
    /// cannot overwrite an imported ES-DE screenshot sharing the plain stem.
    #[tokio::test]
    async fn a_screenshot_set_falls_back_to_the_plain_stem_when_offline() {
        let root = scratch("shot-set");
        let dir = root.join("snes").join(SCREENSHOTS);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Zelda.png"), b"img").unwrap();

        let got = ensure_set(None, &root, "snes", "Zelda", &[]).await;
        assert_eq!(got.len(), 1, "the local screenshot is still found with no server list");
    }
}
