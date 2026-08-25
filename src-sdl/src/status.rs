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
    pub fn parts(&self) -> Vec<Part> {
        let mut out = Vec::new();
        if let Some(clock) = &self.clock {
            out.push(Part::Text(clock.clone()));
        }
        if let Some(bars) = self.bars {
            out.push(Part::Wifi(bars));
        }
        if let Some(pct) = self.battery {
            out.push(Part::Text(format!(
                "{}{pct}%",
                if self.charging { "\u{26a1}" } else { "" }
            )));
        }
        out
    }
}

/// One thing in the corner.
///
/// Signal is not text. It used to be four block characters — the bars a phone
/// shows for a cellular network — and Wi-Fi has been drawn as arcs spreading
/// from a point for as long as anyone has drawn it. Nothing in a font does
/// that at four strengths, so it is a picture, made here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Part {
    Text(String),
    /// 0 to 3 arcs lit.
    Wifi(u8),
}

/// How wide and tall the signal picture is drawn, in pixels.
///
/// Odd width so the arcs have a centre column to be symmetrical about. Small,
/// because the whole corner is: on a 640-point panel the clock beside it is ten
/// points tall.
pub const WIFI_SIZE: (u32, u32) = (13, 10);

/// The signal symbol, as pixels: arcs spreading up from a dot.
///
/// Drawn rather than typed. Each arc is a band of radius around the dot, cut to
/// a wedge either side of vertical, and softened across a pixel at every edge —
/// at this size a hard edge is a staircase and there are only ten rows to make
/// the shape out of.
///
/// Unlit arcs stay, faintly. An icon that loses its outline as the signal drops
/// reads as a different icon rather than as the same one saying less.
pub fn wifi_pixels(bars: u8) -> Vec<u8> {
    let (w, h) = WIFI_SIZE;
    let (cx, cy) = ((w as f32 - 1.0) / 2.0, h as f32 - 1.0);
    // The dot, then three bands. Each is (inner, outer) radius.
    let bands = [(0.0, 1.6), (3.0, 4.3), (5.4, 6.7), (7.8, 9.1)];
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            // Sampled at the pixel's middle, or everything sits half a pixel
            // up and to the left of where it was meant to.
            let (dx, dy) = (x as f32 + 0.5 - cx, cy - (y as f32 + 0.5));
            let r = (dx * dx + dy * dy).sqrt();
            let mut alpha: f32 = 0.0;
            for (i, (inner, outer)) in bands.iter().enumerate() {
                // The wedge: 45 degrees either side of straight up. The dot is
                // the whole circle, having no direction to point in.
                if i > 0 && (dy <= 0.0 || dx.abs() > dy) {
                    continue;
                }
                // Coverage across one pixel at each edge of the band.
                let lit = smooth(r, inner - 0.5, inner + 0.5) * (1.0 - smooth(r, outer - 0.5, outer + 0.5));
                // An arc beyond the strength is drawn faintly rather than not
                // at all.
                let strength = if i == 0 || i as u8 <= bars { 1.0 } else { 0.22 };
                alpha = alpha.max(lit * strength);
            }
            let a = (alpha.clamp(0.0, 1.0) * 255.0) as u8;
            out.extend_from_slice(&[255, 255, 255, a]);
        }
    }
    out
}

/// 0 below `from`, 1 above `to`, an S-curve between.
fn smooth(v: f32, from: f32, to: f32) -> f32 {
    if to <= from {
        return if v >= to { 1.0 } else { 0.0 };
    }
    let t = ((v - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
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
        let text = |p: &Part| match p {
            Part::Text(t) => t.clone(),
            Part::Wifi(n) => format!("wifi{n}"),
        };
        assert_eq!(
            s.parts().iter().map(text).collect::<Vec<_>>(),
            ["00:20"],
            "no radio and no battery means no room taken"
        );

        s.battery = Some(87);
        s.charging = true;
        s.bars = Some(4);
        let parts = s.parts();
        assert_eq!(text(&parts[0]), "00:20");
        assert_eq!(parts.len(), 3);
        // Signal is a picture, not a word — see `Part::Wifi`.
        assert_eq!(parts[1], Part::Wifi(4));
        assert!(text(&parts[2]).contains("87%"));
        assert!(
            text(&parts[2]).starts_with('\u{26a1}'),
            "charging is not marked"
        );

        s.charging = false;
        assert_eq!(text(&s.parts()[2]), "87%");
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
