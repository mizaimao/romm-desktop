// How big things are drawn, and how many columns the window has room for.
//
// One front end has to serve a 4" 960x720 handheld and a 27" desktop, and a
// layout written in pixels cannot do both: the same 150 pixels is a comfortable
// cover on one and a postage stamp on the other. So nothing is written in
// pixels. A card is 150 *points*, and a single scale turns points into pixels
// for whatever it is being drawn on.
//
// Pixels are a measurement here, never a design unit. The moment a number in a
// view means pixels, that view has picked a screen.

use serde::{Deserialize, Serialize};

/// Pixels per point.
///
/// 96 dots per inch is one pixel per point — the baseline every desktop
/// toolkit uses, and what a 1x display has always been.
pub const BASELINE_DPI: f32 = 96.0;

/// Sensible bounds. Below the first, text stops being legible; above the
/// second, something has reported nonsense — a display that claims 2,000 DPI
/// is a driver bug, not a screen, and it should not blow the layout up.
const MIN_SCALE: f32 = 0.5;
const MAX_SCALE: f32 = 4.0;

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
pub struct Scale(f32);

impl Default for Scale {
    fn default() -> Self {
        Scale(1.0)
    }
}

impl Scale {
    /// One point is one pixel.
    pub const NONE: Scale = Scale(1.0);

    /// Whatever was asked for, clamped and snapped.
    ///
    /// Snapped to quarter steps because a scale of 1.37 puts every border and
    /// every glyph on a fraction of a pixel, which reads as a soft, slightly
    /// wrong picture rather than as a bug anybody reports.
    pub fn new(factor: f32) -> Self {
        if !factor.is_finite() {
            return Scale(1.0);
        }
        Scale((factor * 4.0).round() / 4.0).clamped()
    }

    fn clamped(self) -> Self {
        Scale(self.0.clamp(MIN_SCALE, MAX_SCALE))
    }

    /// From what the display says about itself.
    ///
    /// A starting point, not an answer. DPI describes the panel and not how
    /// far away somebody's face is, and that is most of what decides how big a
    /// thing should be: a handheld is held at arm's length or closer, so the
    /// same physical size that is comfortable on a monitor is wastefully large
    /// on it. Hence [`Scale::viewed_from`], and hence the config override — a
    /// handheld should state its scale rather than infer one.
    pub fn from_dpi(dpi: f32) -> Self {
        Scale::new(dpi / BASELINE_DPI)
    }

    /// From the panel and how far away it is held, in the same unit.
    ///
    /// A 27" monitor at 60cm and a 4" handheld at 30cm want the same *angular*
    /// size, which is half the physical size on the closer one. This is that
    /// correction, and it is why the handheld does not simply get its DPI
    /// ratio: a 4" 960x720 panel is around 300 DPI, which would be a scale of
    /// three and a screen with room for two cards on it.
    pub fn viewed_from(dpi: f32, distance_cm: f32) -> Self {
        const REFERENCE_CM: f32 = 60.0;
        // Zero, negative, or NaN — none of which is a distance. `<= 0.0` is
        // false for NaN, so it is spelled out rather than negated.
        if distance_cm <= 0.0 || distance_cm.is_nan() {
            return Scale::from_dpi(dpi);
        }
        Scale::new((dpi / BASELINE_DPI) * (distance_cm / REFERENCE_CM))
    }

    pub fn factor(self) -> f32 {
        self.0
    }

    /// Points to pixels, for handing a number to a renderer.
    pub fn px(self, points: f32) -> f32 {
        points * self.0
    }

    /// Pixels to points, for taking a number back off one — a window size, a
    /// mouse position, a measured glyph.
    pub fn pt(self, pixels: f32) -> f32 {
        pixels / self.0
    }
}

/// The drawable area, in the units a layout is written in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    pub width_px: f32,
    pub height_px: f32,
    pub scale: Scale,
}

impl Viewport {
    pub fn new(width_px: f32, height_px: f32, scale: Scale) -> Self {
        Viewport { width_px, height_px, scale }
    }

    pub fn width(self) -> f32 {
        self.scale.pt(self.width_px)
    }

    pub fn height(self) -> f32 {
        self.scale.pt(self.height_px)
    }

    /// Wider than it is tall, and by how much. 4:3 is 1.33, 16:9 is 1.78.
    pub fn aspect(self) -> f32 {
        if self.height_px > 0.0 { self.width_px / self.height_px } else { 1.0 }
    }

    pub fn panes(self) -> Panes {
        Panes::fitting(self.width())
    }
}

/// How many columns there is room for.
///
/// Room for, not how many are shown: which arrangement to use is a preference,
/// and this is the ceiling it lives under. A window that cannot hold three
/// columns must not be given three however the preference reads, and a
/// handheld never can.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum Panes {
    /// The list you pick from, or the games, or the preview — one at a time,
    /// with a back button. Every handheld, and a narrow window.
    One,
    /// The picker beside the games.
    Two,
    /// Picker, games, preview.
    Three,
}

impl Panes {
    /// Measured off the stylesheet the webview uses, so the two agree about
    /// when a window is wide enough: the picker column is 260 points and the
    /// preview 320, and the middle needs at least two 150-point covers with
    /// 14-point gaps between and around them — about 340. That is 920 for
    /// three, and 600 for two. Rounded up, because a layout that only just
    /// fits is one nobody enjoys.
    pub const TWO_AT: f32 = 660.0;
    pub const THREE_AT: f32 = 1000.0;

    pub fn fitting(width_points: f32) -> Panes {
        if width_points >= Self::THREE_AT {
            Panes::Three
        } else if width_points >= Self::TWO_AT {
            Panes::Two
        } else {
            Panes::One
        }
    }

    pub fn count(self) -> usize {
        match self {
            Panes::One => 1,
            Panes::Two => 2,
            Panes::Three => 3,
        }
    }

    /// The most this window can give, and no more than was asked for.
    pub fn at_most(self, wanted: Panes) -> Panes {
        self.min(wanted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The handheld, at the size the whole exercise is about.
    const POCKET: (f32, f32) = (960.0, 720.0);

    #[test]
    fn points_and_pixels_are_the_same_thing_at_one() {
        let s = Scale::NONE;
        assert_eq!(s.px(150.0), 150.0);
        assert_eq!(s.pt(150.0), 150.0);
    }

    #[test]
    fn points_survive_a_round_trip_through_pixels() {
        for factor in [0.5, 1.0, 1.25, 2.0, 3.0] {
            let s = Scale::new(factor);
            assert!((s.pt(s.px(150.0)) - 150.0).abs() < 0.001, "at {factor}");
        }
    }

    /// A scale of 1.37 puts every border on a fraction of a pixel, which reads
    /// as a soft picture rather than as a bug anybody reports.
    #[test]
    fn a_scale_is_snapped_to_a_quarter() {
        assert_eq!(Scale::new(1.37).factor(), 1.25);
        assert_eq!(Scale::new(1.6).factor(), 1.5);
        assert_eq!(Scale::new(2.0).factor(), 2.0);
    }

    /// A display that claims 2,000 DPI is a driver bug, not a screen.
    #[test]
    fn nonsense_does_not_blow_the_layout_up() {
        assert_eq!(Scale::from_dpi(20_000.0).factor(), MAX_SCALE);
        assert_eq!(Scale::from_dpi(1.0).factor(), MIN_SCALE);
        // Neither of these is a large scale — they are a number that was
        // never computed. Falling back to 1 draws something; clamping to the
        // maximum draws one enormous card and looks deliberate.
        assert_eq!(Scale::new(f32::NAN).factor(), 1.0);
        assert_eq!(Scale::new(f32::INFINITY).factor(), 1.0);
    }

    #[test]
    fn a_plain_desktop_display_is_unscaled() {
        assert_eq!(Scale::from_dpi(96.0), Scale::NONE);
    }

    /// The correction that stops a handheld getting a scale of three.
    ///
    /// A 4" 960x720 panel is about 300 DPI, and at arm's length that would
    /// leave a screen 320 points wide — room for two covers. Held at half the
    /// distance of a monitor, it wants half the physical size.
    #[test]
    fn a_handheld_is_held_closer_and_is_scaled_for_it() {
        let naive = Scale::from_dpi(300.0);
        let held = Scale::viewed_from(300.0, 30.0);
        assert!(held.factor() < naive.factor(), "holding it closer made things bigger");
        assert_eq!(held.factor(), 1.5);

        let screen = Viewport::new(POCKET.0, POCKET.1, held);
        assert_eq!(screen.width(), 640.0);
        assert!(screen.width() > 4.0 * 150.0, "no room for a row of covers");
    }

    /// A monitor at the reference distance is unaffected by the correction.
    #[test]
    fn a_desktop_at_arms_length_is_left_alone() {
        assert_eq!(Scale::viewed_from(96.0, 60.0), Scale::NONE);
        assert_eq!(Scale::viewed_from(220.0, 60.0), Scale::from_dpi(220.0));
    }

    #[test]
    fn a_distance_of_nothing_falls_back_rather_than_dividing_by_zero() {
        assert_eq!(Scale::viewed_from(96.0, 0.0), Scale::from_dpi(96.0));
        assert_eq!(Scale::viewed_from(96.0, -5.0), Scale::from_dpi(96.0));
    }

    /// The one that matters: the handheld is never given three columns.
    #[test]
    fn the_handheld_is_one_pane() {
        let screen = Viewport::new(POCKET.0, POCKET.1, Scale::viewed_from(300.0, 30.0));
        assert_eq!(screen.panes(), Panes::One);
    }

    #[test]
    fn a_desktop_window_earns_its_columns_by_width() {
        let at = |w: f32| Viewport::new(w, 900.0, Scale::NONE).panes();
        assert_eq!(at(500.0), Panes::One);
        assert_eq!(at(800.0), Panes::Two);
        assert_eq!(at(1400.0), Panes::Three);
        // And the boundaries are where they say they are.
        assert_eq!(at(Panes::TWO_AT), Panes::Two);
        assert_eq!(at(Panes::TWO_AT - 1.0), Panes::One);
        assert_eq!(at(Panes::THREE_AT), Panes::Three);
        assert_eq!(at(Panes::THREE_AT - 1.0), Panes::Two);
    }

    /// A retina window is the same *size* as a plain one, in points — which is
    /// the whole reason for the unit. Twice the pixels must not mean twice the
    /// columns.
    #[test]
    fn twice_the_pixels_is_not_twice_the_layout() {
        let plain = Viewport::new(1200.0, 800.0, Scale::NONE);
        let retina = Viewport::new(2400.0, 1600.0, Scale::new(2.0));
        assert_eq!(plain.width(), retina.width());
        assert_eq!(plain.panes(), retina.panes());
        assert!((plain.aspect() - retina.aspect()).abs() < 0.001);
    }

    /// Room for is not the same as shown. A wide window with the preference
    /// set to one pane gets one; a narrow one asking for three still gets one.
    #[test]
    fn the_window_is_a_ceiling_and_the_preference_lives_under_it() {
        assert_eq!(Panes::Three.at_most(Panes::One), Panes::One);
        assert_eq!(Panes::One.at_most(Panes::Three), Panes::One);
        assert_eq!(Panes::Three.at_most(Panes::Three), Panes::Three);
        assert_eq!(Panes::Two.at_most(Panes::Three), Panes::Two);
    }
}
