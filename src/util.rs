//! Small helpers shared across the CLI, TUI and GUI.
//!
//! Each of these existed in two or three places before; keeping one copy means
//! sizes are formatted identically everywhere and `~` expands the same way in
//! every config field.

use std::path::{Path, PathBuf};

/// Human-readable byte size, e.g. `1.5 GB`.
pub fn human(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    for (i, unit) in UNITS.iter().enumerate() {
        if value < 1024.0 || i == UNITS.len() - 1 {
            return format!("{value:.1} {unit}");
        }
        value /= 1024.0;
    }
    unreachable!()
}

/// Expand a leading `~/` against `$HOME`. Other paths pass through unchanged.
pub fn expand_tilde(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => std::env::var_os("HOME")
            .map(|home| PathBuf::from(home).join(rest))
            .unwrap_or_else(|| PathBuf::from(path)),
        None => PathBuf::from(path),
    }
}

/// Total size of everything under `dir`. Missing directories count as zero, so
/// callers can ask about a cache that does not exist yet.
pub fn dir_size(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| match entry.file_type() {
            Ok(t) if t.is_dir() => dir_size(&entry.path()),
            Ok(_) => entry.metadata().map(|m| m.len()).unwrap_or(0),
            Err(_) => 0,
        })
        .sum()
}

/// Build an HTTP client, installing the TLS backend on first use.
///
/// We select rustls without a bundled provider (`rustls-no-provider`) so the
/// build does not drag in `aws-lc-sys`, a large C library whose build script
/// needs a full cross toolchain and therefore breaks Windows and Linux builds.
/// The cost is that *something* must install a provider before the first
/// client is created — reqwest panics otherwise — so every client in this
/// project is built here.
pub fn http_client(timeout: Option<std::time::Duration>) -> anyhow::Result<reqwest::Client> {
    static TLS: std::sync::Once = std::sync::Once::new();
    TLS.call_once(|| {
        // Only errors if a provider is already installed, which is harmless.
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
    let mut b = reqwest::Client::builder()
        .user_agent(concat!("romm-desktop/", env!("CARGO_PKG_VERSION")));
    if let Some(d) = timeout {
        b = b.timeout(d);
    }
    b.build().map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_scales() {
        assert_eq!(human(0), "0.0 B");
        assert_eq!(human(1536), "1.5 KB");
        assert_eq!(human(1_610_612_736), "1.5 GB");
    }

    #[test]
    fn tilde_only_expands_a_leading_home_reference() {
        assert_eq!(expand_tilde("/tmp/x"), PathBuf::from("/tmp/x"));
        assert_eq!(expand_tilde("relative/x"), PathBuf::from("relative/x"));
        // A bare "~" is not a prefix we expand, matching shell-free tools.
        assert_eq!(expand_tilde("~notuser/x"), PathBuf::from("~notuser/x"));
    }
}
