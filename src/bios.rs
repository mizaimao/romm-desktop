//! BIOS files, fetched from the server into the visible library folder.
//!
//! Neo Geo, PlayStation, the MAME family and a dozen others refuse to start
//! without one, and the failure is opaque: the core loads, the screen stays
//! black or shows a line about a missing file, and nothing says which file or
//! where it should go.
//!
//! RomM keeps the BIOS set alongside the games, which is the point — the same
//! server that has the ROMs has the files needed to run them, so a second
//! machine is one sync away rather than a manual hunt.
//!
//! Everything lands flat in `<library>/system/`, not in the folders the server
//! groups them under. RetroArch looks in exactly one directory and does not
//! recurse, so `bios/3do/panafz1.bin` on the server has to become
//! `system/panafz1.bin` here.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::api::{Client, Firmware};

/// What a sync did.
#[derive(Debug, Default)]
pub struct Summary {
    pub downloaded: usize,
    pub already_had: usize,
    pub failed: usize,
    pub bytes: u64,
    pub notes: Vec<String>,
}

impl Summary {
    pub fn headline(&self) -> String {
        if self.downloaded == 0 && self.failed == 0 {
            return format!("BIOS already complete ({} files)", self.already_had);
        }
        let mut s = format!(
            "{} downloaded ({}), {} already present",
            self.downloaded,
            crate::util::human(self.bytes),
            self.already_had
        );
        if self.failed > 0 {
            s.push_str(&format!(", {} failed", self.failed));
        }
        s
    }
}

/// Where BIOS live locally.
pub fn system_dir(library_root: &Path) -> PathBuf {
    library_root.join("system")
}

/// Is this file already here and intact?
///
/// Size first because it is free, then the hash — a truncated download has the
/// wrong size and a corrupted one does not, and only the second needs reading
/// the file back.
fn already_have(path: &Path, want: &Firmware) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if want.file_size_bytes > 0 && meta.len() != want.file_size_bytes as u64 {
        return false;
    }
    match want.md5_hash.as_deref().filter(|h| !h.is_empty()) {
        Some(want_md5) => crate::download::hash_file(path)
            .map(|(md5, _)| md5.eq_ignore_ascii_case(want_md5))
            .unwrap_or(false),
        // No published hash: size is all there is to go on.
        None => true,
    }
}

/// Download every BIOS the server has that is not already here.
///
/// `progress` is called with `(done, total, name)` so a frontend can show which
/// file it is on — the set is 67 files on this server and a bare spinner says
/// nothing about how far through it is.
pub async fn sync(
    client: &Client,
    library_root: &Path,
    mut progress: impl FnMut(usize, usize, &str),
) -> Result<Summary> {
    let list = client
        .firmware()
        .await
        .context("listing BIOS files (does the token have firmware.read?)")?;

    let dest = system_dir(library_root);
    std::fs::create_dir_all(&dest)
        .with_context(|| format!("creating {}", dest.display()))?;

    let mut summary = Summary::default();
    let total = list.len();

    for (i, fw) in list.iter().enumerate() {
        if fw.file_name.is_empty() {
            continue;
        }
        // A server-side name is not a path component to trust blindly.
        let leaf = Path::new(&fw.file_name)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| fw.file_name.clone());
        let path = dest.join(&leaf);
        progress(i + 1, total, &leaf);

        if already_have(&path, fw) {
            summary.already_had += 1;
            continue;
        }

        match client.firmware_content(fw.id, &fw.file_name).await {
            Ok(bytes) => {
                // Written to a temporary name and renamed, so an interrupted
                // download cannot leave a half a BIOS looking like a whole one
                // — which fails in the emulator rather than here.
                let part = dest.join(format!("{leaf}.part"));
                if let Err(e) = std::fs::write(&part, &bytes) {
                    summary.failed += 1;
                    summary.notes.push(format!("{leaf}: {e}"));
                    continue;
                }
                match std::fs::rename(&part, &path) {
                    Ok(()) => {
                        summary.downloaded += 1;
                        summary.bytes += bytes.len() as u64;
                    }
                    Err(e) => {
                        std::fs::remove_file(&part).ok();
                        summary.failed += 1;
                        summary.notes.push(format!("{leaf}: {e}"));
                    }
                }
            }
            Err(e) => {
                summary.failed += 1;
                summary.notes.push(format!(
                    "{leaf}: {}",
                    e.to_string().lines().next().unwrap_or("failed")
                ));
            }
        }
    }
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("romm-bios-{name}"));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn fw(name: &str, size: i64, md5: Option<&str>) -> Firmware {
        Firmware {
            id: 1,
            file_name: name.to_owned(),
            file_size_bytes: size,
            md5_hash: md5.map(str::to_owned),
            sha1_hash: None,
            file_path: None,
            is_verified: true,
        }
    }

    /// Everything goes flat into one directory. RetroArch looks in exactly one
    /// place and does not recurse, so reproducing the server's `bios/3do/`
    /// grouping would hide every file from the emulator that needs it.
    #[test]
    fn bios_land_flat_in_the_system_directory() {
        let root = Path::new("/lib");
        assert_eq!(system_dir(root), Path::new("/lib/system"));
    }

    /// A file already here with the right size and hash is not fetched again —
    /// this runs against 67 files and most of them will not have changed.
    #[test]
    fn an_intact_file_is_recognised() {
        let dir = scratch("intact");
        let path = dir.join("bios.bin");
        std::fs::write(&path, b"hello").unwrap();
        // md5 of "hello"
        let md5 = "5d41402abc4b2a76b9719d911017c592";

        assert!(already_have(&path, &fw("bios.bin", 5, Some(md5))));
        assert!(!already_have(&path, &fw("bios.bin", 5, Some("0".repeat(32).as_str()))));
    }

    /// Wrong size is enough on its own, and is checked first because it costs
    /// nothing — a truncated download does not need its hash computing.
    #[test]
    fn a_truncated_file_is_refetched() {
        let dir = scratch("short");
        let path = dir.join("bios.bin");
        std::fs::write(&path, b"hel").unwrap();
        assert!(!already_have(&path, &fw("bios.bin", 5, None)));
    }

    /// Without a published hash there is nothing to check but the size, which
    /// must still be enough to avoid downloading the whole set every run.
    #[test]
    fn a_file_with_no_published_hash_is_accepted_on_size() {
        let dir = scratch("nohash");
        let path = dir.join("bios.bin");
        std::fs::write(&path, b"hello").unwrap();
        assert!(already_have(&path, &fw("bios.bin", 5, None)));
        assert!(!already_have(&path, &fw("bios.bin", 999, None)));
    }

    /// A file that is not there at all is not "already had".
    #[test]
    fn a_missing_file_is_not_mistaken_for_a_present_one() {
        let dir = scratch("absent");
        assert!(!already_have(&dir.join("nope.bin"), &fw("nope.bin", 5, None)));
    }

    #[test]
    fn the_headline_says_nothing_happened_when_nothing_did() {
        let s = Summary { already_had: 67, ..Default::default() };
        assert_eq!(s.headline(), "BIOS already complete (67 files)");
        let s = Summary { downloaded: 3, already_had: 64, bytes: 1024, failed: 1, ..Default::default() };
        assert!(s.headline().contains("3 downloaded"));
        assert!(s.headline().contains("1 failed"));
    }
}
