// The clock, the signal and the battery, for the corner of the screen.
//
// Everything here is a file read, so it is cheap — but it is not *free*, and
// the loop runs at the display's refresh rate. Reading four sysfs files sixty
// times a second to draw a number that changes every few minutes is the kind of
// thing that shows up as battery drain and nothing else, so it is polled on a
// timer and cached in between.
//
// Which files those are is the platform's business, not this module's: see
// `romm_desktop::platform`. On a Mac none of them exist and every field comes
// back `None`, which is why the preview needs `ROMM_SDL_STATUS=fake` to show
// what the handheld will.

use romm_desktop::platform;

/// How often to look. Battery moves in percent and Wi-Fi in bars; neither is
/// worth a read per frame.
const EVERY_MS: u64 = 2_000;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Status {
    /// `HH:MM`, or `None` when the clock cannot be read.
    pub clock: Option<String>,
    /// Signal as 0..=4 bars, or `None` when there is no wireless at all.
    pub bars: Option<u8>,
    /// Charge as a percentage.
    pub battery: Option<u8>,
    /// Whether something is plugged in.
    pub charging: bool,
    next_read: u64,
}

impl Status {
    /// Refresh if it is time to. `now` is the loop's millisecond clock.
    pub fn poll(&mut self, now: u64) {
        if now < self.next_read {
            return;
        }
        self.next_read = now + EVERY_MS;
        self.clock = romm_desktop::util::local_hhmm();

        if std::env::var("ROMM_SDL_STATUS").as_deref() == Ok("fake") {
            // The Mac has no battery node and no wlan0. Without this the
            // preview shows a clock and two gaps, and the corner cannot be
            // judged until it is on the device.
            self.bars = Some(3);
            self.battery = Some(72);
            self.charging = true;
            return;
        }

        let p = platform::current();
        self.bars = p.wifi().and_then(|w| bars_from_proc(&w));
        if let Some(b) = p.battery() {
            self.battery = read_number(&b.capacity).map(|n| n.min(100) as u8);
            self.charging = b.charging.iter().any(|p| read_number(p) == Some(1));
        }
    }

    /// What the corner shows, right to left. Empty when there is nothing to say.
    pub fn parts(&self) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(clock) = &self.clock {
            out.push(clock.clone());
        }
        if let Some(bars) = self.bars {
            out.push(signal_glyph(bars));
        }
        if let Some(pct) = self.battery {
            out.push(format!(
                "{}{pct}%",
                if self.charging { "\u{26a1}" } else { "" }
            ));
        }
        out
    }
}

/// Signal as blocks, because a 640-point panel has no room for an icon set and
/// a glyph reads at a glance the way a number does not.
fn signal_glyph(bars: u8) -> String {
    match bars {
        0 => "\u{2581}".to_owned(),
        1 => "\u{2581}\u{2583}".to_owned(),
        2 => "\u{2581}\u{2583}\u{2585}".to_owned(),
        _ => "\u{2581}\u{2583}\u{2585}\u{2587}".to_owned(),
    }
}

/// Read a whole number out of a one-line sysfs file.
fn read_number(path: &std::path::Path) -> Option<i64> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// Pull the link quality for one interface out of `/proc/net/wireless`.
///
/// The file is two header lines then one row per interface:
///
/// ```text
///  wlan0: 0000   40.   48.    0.  ...
/// ```
///
/// The third column is the quality — 40 of a possible 70 on the Flip while
/// connected. It carries a trailing dot, which is why it is trimmed rather than
/// parsed directly.
fn bars_from_proc(w: &platform::Wifi) -> Option<u8> {
    let text = std::fs::read_to_string(&w.proc_wireless).ok()?;
    let quality = quality_for(&text, w.interface)?;
    let fraction = quality as f32 / w.max_quality.max(1) as f32;
    Some((fraction * 4.0).round().clamp(0.0, 4.0) as u8)
}

/// Split out from the file read so it can be tested against a real capture
/// without a wireless card in the machine running the tests.
fn quality_for(text: &str, interface: &str) -> Option<u32> {
    for line in text.lines() {
        let line = line.trim_start();
        let Some(rest) = line.strip_prefix(interface) else {
            continue;
        };
        let Some(rest) = rest.strip_prefix(':') else {
            continue;
        };
        // status, then quality.
        let mut fields = rest.split_whitespace();
        let _status = fields.next()?;
        let quality = fields.next()?.trim_end_matches('.');
        return quality.parse().ok();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real capture from the Flip, connected to Wi-Fi on 2026-08-25.
    const PROC: &str = "Inter-| sta-|   Quality        |   Discarded packets               | Missed | WE\n \
                        face | tus | link level noise |  nwid  crypt   frag  retry   misc | beacon | 22\n \
                        wlan0: 0000   40.   48.    0.       0      0      0      0      0        0\n";

    /// The quality column is the third field and carries a trailing dot, which
    /// is what stops it parsing if it is read as-is.
    #[test]
    fn the_quality_is_read_from_a_real_capture() {
        assert_eq!(quality_for(PROC, "wlan0"), Some(40));
    }

    /// An interface that is not in the file is no signal, not a panic and not a
    /// zero — zero bars means connected and weak, which is a different thing.
    #[test]
    fn an_absent_interface_reports_nothing() {
        assert_eq!(quality_for(PROC, "wlan1"), None);
        assert_eq!(quality_for("", "wlan0"), None);
        assert_eq!(quality_for("nonsense\n", "wlan0"), None);
    }

    /// The header lines mention neither interface and must not be mistaken for
    /// one — "face |" begins a line and ends in a colon further along.
    #[test]
    fn the_header_is_not_read_as_an_interface() {
        assert_eq!(quality_for(PROC, "face"), None);
    }

    /// 40 of 70 is a bit over half, which is three bars of four.
    #[test]
    fn quality_becomes_bars() {
        let w = platform::Wifi {
            proc_wireless: "/nonexistent".into(),
            interface: "wlan0",
            max_quality: 70,
            helper: None,
            settings_get: None,
        };
        assert_eq!(bars_from_proc(&w), None, "a missing file is no signal");
        // The arithmetic, without the file.
        let bars = |q: f32| (q / 70.0 * 4.0).round() as u8;
        assert_eq!(bars(40.0), 2);
        assert_eq!(bars(70.0), 4);
        assert_eq!(bars(0.0), 0);
    }

    /// Charging shows a bolt; not charging shows the number alone.
    #[test]
    fn the_corner_says_what_it_knows_and_no_more() {
        let mut s = Status {
            clock: Some("00:20".into()),
            ..Status::default()
        };
        assert_eq!(
            s.parts(),
            ["00:20"],
            "no radio and no battery means no room taken"
        );

        s.battery = Some(87);
        s.charging = true;
        s.bars = Some(4);
        let parts = s.parts();
        assert_eq!(parts[0], "00:20");
        assert_eq!(parts.len(), 3);
        assert!(parts[2].contains("87%"));
        assert!(parts[2].starts_with('\u{26a1}'), "charging is not marked");

        s.charging = false;
        assert_eq!(s.parts()[2], "87%");
    }

    /// Polling is on a timer. Sixty reads a second of four sysfs files, for a
    /// number that moves in percent, is battery spent to say so.
    #[test]
    fn it_does_not_read_the_files_every_frame() {
        let mut s = Status::default();
        s.poll(0);
        let first = s.next_read;
        assert!(first >= EVERY_MS, "the next read was not pushed out");
        s.clock = Some("wrong".into());
        s.poll(first - 1);
        assert_eq!(s.clock.as_deref(), Some("wrong"), "it read again too early");
        s.poll(first);
        assert_ne!(s.clock.as_deref(), Some("wrong"), "it never read again");
    }
}
