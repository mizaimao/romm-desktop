//! Stage 4 — ROM download with resume and hash verification.
//!
//! Large transfers run ~3.8 MB/s (PLAN.md §3), so a 4.7 GB PSP image is a
//! ~20 minute download. Resume is not a nicety here: it is the difference
//! between an interrupted transfer costing seconds or twenty minutes.
//!
//! Bytes land in `<name>.part` and are only renamed into place once the hash
//! matches, so a partial file can never be mistaken for a complete ROM.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use futures_util::StreamExt;
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

fn hash_file(path: &Path) -> Result<(String, String)> {
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

/// Compare against whichever hash the server published. Returns which one was
/// used, or an error describing the mismatch.
fn verify(path: &Path, target: &Target<'_>) -> Result<Verified> {
    let want_md5 = target.md5.filter(|h| !h.is_empty());
    let want_sha1 = target.sha1.filter(|h| !h.is_empty());
    if want_md5.is_none() && want_sha1.is_none() {
        return Ok(Verified::SizeOnly);
    }
    let (got_md5, got_sha1) = hash_file(path)?;
    if let Some(want) = want_md5 {
        if got_md5.eq_ignore_ascii_case(want) {
            return Ok(Verified::Md5);
        }
        bail!("md5 mismatch: server {want}, downloaded {got_md5}");
    }
    let want = want_sha1.unwrap();
    if got_sha1.eq_ignore_ascii_case(want) {
        return Ok(Verified::Sha1);
    }
    bail!("sha1 mismatch: server {want}, downloaded {got_sha1}")
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

    if final_path.is_file() {
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

    if let Some(expected) = target.expected_size
        && written != expected
    {
        bail!(
            "size mismatch: expected {expected} bytes, got {written} \
             (partial left at {})",
            part_path.display()
        );
    }

    let verified = match verify(&part_path, target) {
        Ok(v) => v,
        Err(e) => {
            // Keep the bad file for inspection rather than silently deleting.
            bail!("{e}\n  bad download left at {}", part_path.display());
        }
    };

    std::fs::rename(&part_path, &final_path)
        .with_context(|| format!("renaming into {}", final_path.display()))?;

    Ok(Outcome::Downloaded {
        path: final_path,
        bytes: written,
        resumed_from: started_at,
        verified,
    })
}
