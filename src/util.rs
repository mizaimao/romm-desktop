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

/// Now, as the server writes it: `2026-08-14T09:30:00`.
///
/// Hand-rolled rather than pulled from a date library, because this is the only
/// date arithmetic in the project and the format has to match what the server
/// already stores. `last_played` is compared and sorted as text, so a locally
/// recorded session written as epoch seconds — or with a timezone suffix the
/// server never uses — would sort either above everything or below it, and the
/// "continue playing" row would be wrong in a way nothing announces.
pub fn now_iso() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    iso_from_epoch(secs)
}

/// UTC, always. A local time would make a library carried between timezones
/// record sessions that appear to happen in the wrong order.
pub fn iso_from_epoch(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Days since 1970-01-01 to a calendar date.
///
/// Howard Hinnant's algorithm, which shifts the year to start in March so the
/// leap day lands at the end and the month-length arithmetic becomes a single
/// expression with no table and no special case for February.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// A duration in words: "18 minutes", "2 h 05 m".
///
/// Seconds are dropped above a minute. Nobody reads "1 h 23 m 07 s", and the
/// figure is a wall-clock measurement of a window being open, which is not
/// accurate to the second in any sense that matters.
pub fn spell_duration(seconds: i64) -> String {
    match seconds {
        s if s < 60 => format!("{s} seconds"),
        s if s < 3600 => format!("{} minutes", s / 60),
        s => format!("{} h {:02} m", s / 3600, (s % 3600) / 60),
    }
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

    /// Checked against dates whose epoch seconds are known independently. The
    /// leap-year rules are the part that goes wrong quietly: 2000 is a leap
    /// year and 1900 is not, and an implementation that gets that backwards is
    /// correct for decades either side.
    #[test]
    fn the_clock_agrees_with_dates_whose_answers_are_known() {
        assert_eq!(iso_from_epoch(0), "1970-01-01T00:00:00");
        assert_eq!(iso_from_epoch(1_000_000_000), "2001-09-09T01:46:40");
        assert_eq!(iso_from_epoch(951_782_400), "2000-02-29T00:00:00");
        assert_eq!(iso_from_epoch(1_709_164_800), "2024-02-29T00:00:00");
        assert_eq!(iso_from_epoch(1_767_225_599), "2025-12-31T23:59:59");
        assert_eq!(iso_from_epoch(1_767_225_600), "2026-01-01T00:00:00");
    }

    /// `last_played` is sorted as text against timestamps the server wrote, so
    /// the format has to be identical: same width, same separator, no zone.
    #[test]
    fn a_locally_recorded_time_sorts_against_the_servers() {
        let ours = iso_from_epoch(1_767_225_600);
        let theirs = "2025-06-01T12:00:00";
        assert_eq!(ours.len(), theirs.len());
        assert!(ours.as_str() > theirs, "{ours} should sort after {theirs}");
        assert!(!ours.contains('Z') && !ours.contains('+'), "the server writes no zone");
    }

    #[test]
    fn durations_read_as_someone_would_say_them() {
        assert_eq!(spell_duration(12), "12 seconds");
        assert_eq!(spell_duration(600), "10 minutes");
        assert_eq!(spell_duration(3600), "1 h 00 m");
        assert_eq!(spell_duration(7_500), "2 h 05 m");
    }
}
