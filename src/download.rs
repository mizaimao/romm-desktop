//! ROM download with resume and hash verification.
//!
//! Transfers run at wire speed (~115 MB/s measured, PLAN.md §3), so resume
//! exists for reliability rather than to rescue a slow transfer: a dropped
//! connection part-way through a 1.5 GB disc image should not start over.
//!
//! Bytes land in `<name>.part` and are only renamed into place once the hash
//! matches, so a partial file can never be mistaken for a complete ROM.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use futures_util::StreamExt;
// md5 and sha1 both implement `digest::Digest`, so importing the trait once
// brings `::digest()` into scope for both.
use md5::Digest as _;

/// Outcome of a download attempt.
#[derive(Debug)]
pub enum Outcome {
    /// Already present and verified; nothing transferred.
    AlreadyHave(PathBuf),
    Downloaded {
        path: PathBuf,
        bytes: u64,
        resumed_from: u64,
        verified: Verified,
    },
}

#[derive(Debug, PartialEq)]
pub enum Verified {
    Md5,
    Sha1,
    /// Server published no hash for this ROM — size was all we could check.
    SizeOnly,
}

/// What we need to fetch and check one ROM.
pub struct Target<'a> {
    pub rom_id: i64,
    pub fs_name: &'a str,
    pub platform_slug: &'a str,
    pub expected_size: Option<u64>,
    pub md5: Option<&'a str>,
    pub sha1: Option<&'a str>,
    /// Folder ROM (multi-disc). The server zips the folder for transfer, so
    /// the bytes on the wire are not the ROM and must be unpacked.
    pub multi_file: bool,
}

fn urlencode_path(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub fn hash_file(path: &Path) -> Result<(String, String)> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut md5 = md5::Md5::new();
    let mut sha1 = sha1::Sha1::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        md5.update(&buf[..n]);
        sha1.update(&buf[..n]);
    }
    Ok((hex::encode(md5.finalize()), hex::encode(sha1.finalize())))
}

/// Fallback exclusion lists, used only until the server's are fetched.
///
/// These are RomM 5.0.0's defaults. They are configurable per deployment, so
/// the live values from `/api/config` take precedence — see [`set_exclusions`].
const FALLBACK_NAMES: &[&str] = &[
    ".DS_Store", ".localized", ".Trashes", ".stfolder", "@SynoResource",
    "gamelist.xml", "metadata.pegasus.txt",
];
const FALLBACK_EXTS: &[&str] =
    &["db", "ini", "tmp", "bak", "lock", "log", "cache", "crdownload"];

/// Exclusions in force for archive hashing.
static EXCLUSIONS: std::sync::OnceLock<(Vec<String>, Vec<String>)> = std::sync::OnceLock::new();

/// Adopt the server's exclusion lists. Call once, before verifying downloads.
///
/// Ignored if already set — the first caller (startup) wins, so a later refresh
/// cannot change hashing semantics mid-run.
pub fn set_exclusions(names: Vec<String>, exts: Vec<String>) {
    if names.is_empty() && exts.is_empty() {
        return;
    }
    let _ = EXCLUSIONS.set((names, exts));
}

fn exclusions() -> (Vec<String>, Vec<String>) {
    EXCLUSIONS.get().cloned().unwrap_or_else(|| {
        (
            FALLBACK_NAMES.iter().map(|s| s.to_string()).collect(),
            FALLBACK_EXTS.iter().map(|s| s.to_string()).collect(),
        )
    })
}

fn excluded(name: &str) -> bool {
    let (names, exts) = exclusions();
    let base = name.rsplit('/').next().unwrap_or(name);
    let lower = base.to_ascii_lowercase();
    exts.iter().any(|e| lower.ends_with(&format!(".{e}")))
        || names.iter().any(|n| base == n)
}

/// Reproduce RomM's composite archive hash.
///
/// RomM does not hash the archive bytes, nor any single member: it streams the
/// decompressed contents of **every** eligible member, in ASCII order of the
/// internal path, through one running digest. From
/// `handler/filesystem/roms_handler.py`:
///
/// ```python
/// for name, size, chunks in ARCHIVE_READERS[rom_ext](...):
///     for chunk in chunks:
///         md5_h.update(chunk)     # one hash across all members
/// ```
///
/// That is why a single-member zip appears to match "the file inside" (with one
/// member the concatenation *is* that member) while multi-member romsets match
/// nothing simpler.
pub fn hash_archive_composite(path: &Path) -> Option<(String, String)> {
    let mut md5 = md5::Md5::new();
    let mut sha1 = sha1::Sha1::new();
    let mut any = false;

    if let Ok(file) = std::fs::File::open(path)
        && let Ok(mut zip) = zip::ZipArchive::new(std::io::BufReader::new(file))
    {
        let mut names: Vec<String> = Vec::new();
        for i in 0..zip.len() {
            if let Ok(e) = zip.by_index(i)
                && !e.is_dir()
            {
                names.push(e.name().to_owned());
            }
        }
        // ASCII order of the full internal path, as the server sorts.
        names.sort();
        for name in names {
            if excluded(&name) {
                continue;
            }
            let Ok(mut entry) = zip.by_name(&name) else { continue };
            let mut buf = Vec::new();
            if std::io::Read::read_to_end(&mut entry, &mut buf).is_ok() {
                md5.update(&buf);
                sha1.update(&buf);
                any = true;
            }
        }
        return any.then(|| (hex::encode(md5.finalize()), hex::encode(sha1.finalize())));
    }

    let mut archive = sevenz_rust2::ArchiveReader::open(path, Default::default()).ok()?;
    let mut names: Vec<String> = archive
        .archive()
        .files
        .iter()
        .filter(|e| e.has_stream())
        .map(|e| e.name().to_owned())
        .collect();
    names.sort();
    for name in names {
        if excluded(&name) {
            continue;
        }
        if let Ok(buf) = archive.read_file(&name) {
            md5.update(&buf);
            sha1.update(&buf);
            any = true;
        }
    }
    any.then(|| (hex::encode(md5.finalize()), hex::encode(sha1.finalize())))
}

/// Per-member hashes, for diagnostics (`hashcheck`).
pub fn hash_archive_members(path: &Path) -> Vec<(String, String, String)> {
    let mut out = Vec::new();

    if let Ok(file) = std::fs::File::open(path)
        && let Ok(mut zip) = zip::ZipArchive::new(std::io::BufReader::new(file))
    {
        for i in 0..zip.len() {
            let Ok(mut entry) = zip.by_index(i) else { continue };
            if entry.is_dir() {
                continue;
            }
            let name = entry.name().to_owned();
            let mut buf = Vec::new();
            if std::io::Read::read_to_end(&mut entry, &mut buf).is_ok() {
                out.push((
                    name,
                    hex::encode(md5::Md5::digest(&buf)),
                    hex::encode(sha1::Sha1::digest(&buf)),
                ));
            }
        }
        return out;
    }

    // 7z: same idea, different container.
    if let Ok(mut archive) = sevenz_rust2::ArchiveReader::open(path, Default::default()) {
        let names: Vec<String> = archive
            .archive()
            .files
            .iter()
            .filter(|e| e.has_stream())
            .map(|e| e.name().to_owned())
            .collect();
        for name in names {
            if let Ok(buf) = archive.read_file(&name) {
                out.push((
                    name,
                    hex::encode(md5::Md5::digest(&buf)),
                    hex::encode(sha1::Sha1::digest(&buf)),
                ));
            }
        }
    }
    out
}

/// Compare against whichever hash the server published. Returns which one was
/// used, or an error describing the mismatch.
fn verify(path: &Path, target: &Target<'_>) -> Result<Verified> {
    let want_md5 = target.md5.filter(|h| !h.is_empty());
    let want_sha1 = target.sha1.filter(|h| !h.is_empty());
    if want_md5.is_none() && want_sha1.is_none() {
        return Ok(Verified::SizeOnly);
    }

    let (got_md5, got_sha1) = hash_file(path)?;
    let matches = |md5: &str, sha1: &str| match (want_md5, want_sha1) {
        (Some(w), _) if md5.eq_ignore_ascii_case(w) => Some(Verified::Md5),
        (None, Some(w)) if sha1.eq_ignore_ascii_case(w) => Some(Verified::Sha1),
        _ => None,
    };

    if let Some(v) = matches(&got_md5, &got_sha1) {
        return Ok(v);
    }
    // Archives: RomM hashes the concatenated contents of every member.
    if let Some((c_md5, c_sha1)) = hash_archive_composite(path)
        && let Some(v) = matches(&c_md5, &c_sha1)
    {
        return Ok(v);
    }
    let members = hash_archive_members(path);

    let want = want_md5.unwrap_or_else(|| want_sha1.unwrap_or(""));
    bail!(
        "hash mismatch: server {want}, downloaded {got_md5}{}",
        if members.is_empty() {
            String::new()
        } else {
            format!(" (composite of {} archive member(s) also checked)", members.len())
        }
    )
}

/// Extract a downloaded folder-ROM zip into `dest`, flattening any wrapper
/// directory the archive carries.
fn unpack_folder_rom(zip_path: &Path, dest: &Path) -> Result<()> {
    let file = std::fs::File::open(zip_path)?;
    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(file))
        .with_context(|| format!("reading {}", zip_path.display()))?;

    if dest.exists() {
        std::fs::remove_dir_all(dest).ok();
    }
    std::fs::create_dir_all(dest)?;

    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        if entry.is_dir() {
            continue;
        }
        // Keep only the leaf: entries may be prefixed with the folder name,
        // and we already have a directory for it.
        let name = Path::new(entry.name())
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        let out = dest.join(&name);
        let mut w = std::fs::File::create(&out)
            .with_context(|| format!("writing {}", out.display()))?;
        std::io::copy(&mut entry, &mut w)?;
    }
    Ok(())
}

/// Download `target` into `<library_roms>/<platform>/<fs_name>`.
///
/// `progress` is called with `(downloaded_so_far, total_or_0)`.
pub async fn fetch(
    http: &reqwest::Client,
    base_url: &str,
    auth: &str,
    target: &Target<'_>,
    library_roms: &Path,
    mut progress: impl FnMut(u64, u64),
) -> Result<Outcome> {
    // A multi-disc playlist whose discs were never indexed is a stub on this
    // server; downloading it yields a file that cannot launch. See PLAN.md §3.
    if target.fs_name.ends_with(".m3u")
        && target.expected_size.is_some_and(|s| s < 4096)
    {
        bail!(
            "{} is a {}-byte playlist stub — its disc images were never indexed by RomM, \
             so there is nothing to download. Fix server-side with convert-to-folder.",
            target.fs_name,
            target.expected_size.unwrap_or(0)
        );
    }

    let dir = library_roms.join(target.platform_slug);
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let final_path = dir.join(target.fs_name);
    let part_path = dir.join(format!("{}.part", target.fs_name));

    if target.multi_file {
        // Hashes describe the folder's contents, not the transferred zip, so
        // presence of the unpacked directory is the check available here.
        if final_path.is_dir()
            && std::fs::read_dir(&final_path).map(|d| d.count() > 0).unwrap_or(false)
        {
            return Ok(Outcome::AlreadyHave(final_path));
        }
    } else if final_path.is_file() {
        // Trust it only if it still matches; otherwise fall through and refetch.
        if verify(&final_path, target).is_ok() {
            return Ok(Outcome::AlreadyHave(final_path));
        }
        std::fs::remove_file(&final_path).ok();
    }

    let resume_from = std::fs::metadata(&part_path).map(|m| m.len()).unwrap_or(0);
    let url = format!(
        "{}/api/roms/{}/content/{}",
        base_url.trim_end_matches('/'),
        target.rom_id,
        urlencode_path(target.fs_name)
    );

    let mut req = http
        .get(&url)
        .header("Authorization", format!("Basic {auth}"));
    if resume_from > 0 {
        req = req.header("Range", format!("bytes={resume_from}-"));
    }
    let resp = req.send().await.with_context(|| format!("GET {url}"))?;
    let status = resp.status();
    if !status.is_success() {
        bail!("GET {url} -> {status}");
    }

    // 206 means the server honoured Range; 200 means it ignored it and is
    // sending the whole file, so any partial data must be discarded.
    let appending = status == reqwest::StatusCode::PARTIAL_CONTENT && resume_from > 0;
    let started_at = if appending { resume_from } else { 0 };

    let total = target
        .expected_size
        .unwrap_or_else(|| resp.content_length().unwrap_or(0) + started_at);

    let mut file = if appending {
        std::fs::OpenOptions::new().append(true).open(&part_path)?
    } else {
        std::fs::File::create(&part_path)?
    };

    let mut written = started_at;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("reading response body")?;
        file.write_all(&chunk)?;
        written += chunk.len() as u64;
        progress(written, total);
    }
    file.flush()?;
    drop(file);

    // A folder ROM is transferred as a zip of its contents, so the bytes on
    // the wire never equal `fs_size_bytes` (which is the sum of the files).
    // Zip overhead alone makes them differ by a few hundred bytes.
    if let Some(expected) = target.expected_size
        && !target.multi_file
        && written != expected
    {
        bail!(
            "size mismatch: expected {expected} bytes, got {written} \
             (partial left at {})",
            part_path.display()
        );
    }

    let verified = if target.multi_file {
        // The server hashed the folder's contents; we hold a zip of them.
        Verified::SizeOnly
    } else {
        match verify(&part_path, target) {
            Ok(v) => v,
            Err(e) => {
                // Keep the bad file for inspection rather than silently deleting.
                bail!("{e}\n  bad download left at {}", part_path.display());
            }
        }
    };

    // Folder ROMs arrive as a zip of the directory. Unpack it so the layout
    // matches the server's and the .m3u inside points at real neighbours.
    if target.multi_file {
        unpack_folder_rom(&part_path, &final_path)?;
        std::fs::remove_file(&part_path).ok();
        return Ok(Outcome::Downloaded {
            path: final_path,
            bytes: written,
            resumed_from: started_at,
            verified,
        });
    }

    std::fs::rename(&part_path, &final_path)
        .with_context(|| format!("renaming into {}", final_path.display()))?;

    Ok(Outcome::Downloaded {
        path: final_path,
        bytes: written,
        resumed_from: started_at,
        verified,
    })
}
