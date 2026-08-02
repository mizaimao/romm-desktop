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
