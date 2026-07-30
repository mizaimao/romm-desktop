//! Downloading libretro cores from the buildbot.
//!
//! Cores ship as `<stem>_libretro.dylib.zip`, one zip per core, rebuilt
//! nightly. Download and unzip is the whole procedure — see PLAN.md §6.

use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result, bail};

const BUILDBOT: &str = "https://buildbot.libretro.com/nightly";

/// Buildbot path segment for the host we're running on.
///
/// Not every core is built for `arm64` (Dolphin has been absent), so callers
/// should be prepared to fall back to `x86_64` under Rosetta.
pub fn platform_segment() -> Result<&'static str> {
    Ok(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "apple/osx/arm64",
        ("macos", "x86_64") => "apple/osx/x86_64",
        ("linux", "x86_64") => "linux/x86_64",
        ("linux", "aarch64") => "linux/arm64",
        (os, arch) => bail!("no buildbot mapping for {os}/{arch}"),
    })
}

/// Shared-library extension for the host.
pub fn lib_extension() -> &'static str {
    match std::env::consts::OS {
        "macos" => "dylib",
        "windows" => "dll",
        _ => "so",
    }
}

pub fn core_filename(core: &str) -> String {
    format!("{core}_libretro.{}", lib_extension())
}

fn core_url(segment: &str, core: &str) -> String {
    format!("{BUILDBOT}/{segment}/latest/{}.zip", core_filename(core))
}

/// Download one core and extract it into `dest_dir`.
///
/// Returns the number of bytes written. Existing files are overwritten, so a
/// re-run upgrades to the current nightly.
pub async fn install(
    client: &reqwest::Client,
    core: &str,
    dest_dir: &Path,
    segment: &str,
) -> Result<u64> {
    let url = core_url(segment, core);
    let resp = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("requesting {url}"))?;

    if !resp.status().is_success() {
        bail!("{} -> HTTP {}", url, resp.status());
    }

    let bytes = resp
        .bytes()
        .await
        .with_context(|| format!("downloading {url}"))?;

    // The archive holds exactly the one shared library.
    let reader = std::io::Cursor::new(&bytes);
    let mut zip = zip::ZipArchive::new(reader)
        .with_context(|| format!("{core}: response was not a zip ({} bytes)", bytes.len()))?;

    let wanted = core_filename(core);
    let mut written = 0u64;
    std::fs::create_dir_all(dest_dir)
        .with_context(|| format!("creating {}", dest_dir.display()))?;

    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        if entry.is_dir() {
            continue;
        }
        // Ignore any path component in the archive; cores are flat.
        let name = Path::new(entry.name())
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if name != wanted {
            continue;
        }
        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut buf)?;
        let out = dest_dir.join(&name);
        std::fs::write(&out, &buf).with_context(|| format!("writing {}", out.display()))?;
        written = buf.len() as u64;
    }

    if written == 0 {
        bail!("{core}: archive did not contain {wanted}");
    }
    Ok(written)
}
