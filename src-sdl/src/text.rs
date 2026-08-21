// Drawing words.
//
// The item docs/handheld-frontend.md calls "the least glamorous here and the
// one most likely to make the result feel cheap", and it is right. A webview
// wraps, ellipsises and falls back across fonts without being asked. Nothing
// below that does any of it for free.
//
// Four separate problems, and skipping any one of them shows:
//
//   * **Shaping.** Turning characters into positioned glyphs. Trivial for
//     English, not for anything with marks or joining forms.
//   * **Fallback.** A library with Japanese titles in it will ask for glyphs
//     no Latin face has. Drawing a row of empty boxes is the failure everyone
//     has seen and nobody reports as a bug, because it looks deliberate.
//   * **Line breaking.** Where a line may be split, which is Unicode's answer
//     and not "at spaces" — CJK has no spaces and breaks almost anywhere.
//   * **Ellipsis.** A title too long for its card has to stop somewhere, and
//     the somewhere has to be a character boundary in the original string, not
//     a convenient-looking gap between glyphs.
//
// `cosmic-text` answers the first three. The fourth is here, because where a
// name gets cut is a decision about this app rather than about Unicode.

use anyhow::Result;
use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, SwashCache, Wrap};
use sdl2::pixels::{Color, PixelFormatEnum};
use sdl2::rect::Rect;
use sdl2::render::{BlendMode, Texture, TextureCreator, WindowCanvas};
use sdl2::video::WindowContext;
use std::collections::HashMap;

/// What a rendered piece of text is asked for.
///
/// Everything in points. The scale turns them into pixels at the last moment,
/// which is also why it is part of the key: the same title at the same size on
/// a different display is a different set of pixels.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Spec {
    pub text: String,
    /// Point size, times a hundred, so the key can be hashed. Nobody asks for
    /// a hundredth of a point.
    size: u32,
    /// The box it has to fit inside, in pixels, or `None` for "as wide as it
    /// likes".
    width: Option<u32>,
    /// How many lines it may take before it is cut short.
    lines: u16,
    scale: u32,
}

impl Spec {
    pub fn new(text: impl Into<String>, size_points: f32, scale: f32) -> Self {
        Spec {
            text: text.into(),
            size: (size_points * 100.0).round() as u32,
            width: None,
            lines: 1,
            scale: (scale * 100.0).round() as u32,
        }
    }

    /// Wrap inside `width` points, over at most `lines` of them.
    pub fn wrapped(mut self, width_points: f32, lines: u16) -> Self {
        self.width = Some((width_points * 100.0).round() as u32);
        self.lines = lines.max(1);
        self
    }

    fn size_px(&self) -> f32 {
        self.size as f32 / 100.0 * self.scale as f32 / 100.0
    }

    fn width_px(&self) -> Option<f32> {
        self.width.map(|w| w as f32 / 100.0 * self.scale as f32 / 100.0)
    }
}

/// A laid-out, rasterised piece of text: coverage, and how big it came out.
///
/// Greyscale rather than colour, because the colour is decided by whatever
/// draws it — a name is dim in one place and bright in another, and
/// rasterising it twice for that would be silly.
pub struct Raster {
    pub width: u32,
    pub height: u32,
    /// One byte of coverage per pixel.
    pub coverage: Vec<u8>,
    /// Whether the text was cut short to fit.
    pub clipped: bool,
}

/// What a line is cut short with. One character, not three dots: the ellipsis
/// is narrower, and it is what every other list in the world uses.
const ELLIPSIS: char = '…';

pub struct Fonts {
    system: FontSystem,
    swash: SwashCache,
}

impl Fonts {
    /// Find the faces the machine has.
    ///
    /// The same directories fontconfig reads, which is how the handheld's
    /// `fonts-noto-cjk` is found without us shipping or naming it — see
    /// docs/handheld-device.md.
    pub fn load() -> Result<Self> {
        let system = FontSystem::new();
        let fonts = Fonts { system, swash: SwashCache::new() };
        if fonts.faces() == 0 {
            anyhow::bail!("no fonts on this machine at all");
        }
        Ok(fonts)
    }

    pub fn faces(&self) -> usize {
        self.system.db().len()
    }

    /// Whether anything installed can draw this string.
    ///
    /// Not a nicety: on a machine with no CJK face a Japanese title silently
    /// becomes a row of boxes, and there is nothing on screen to say the font
    /// is missing rather than the name being wrong.
    pub fn can_draw(&mut self, text: &str) -> bool {
        let laid = self.lay_out(&Spec::new(text, 16.0, 1.0));
        laid.glyphs > 0 && laid.missing == 0
    }

    fn lay_out(&mut self, spec: &Spec) -> Laid {
        let size = spec.size_px();
        // Line height at 1.3x. Tighter and diacritics collide with the line
        // above; looser and a two-line title reads as two titles.
        let mut buffer = Buffer::new(&mut self.system, Metrics::new(size, size * 1.3));
        let mut buffer = buffer.borrow_with(&mut self.system);
        buffer.set_wrap(if spec.width_px().is_some() { Wrap::WordOrGlyph } else { Wrap::None });
        buffer.set_size(spec.width_px(), None);
        // `Shaping::Advanced` rather than `Basic`: basic skips the shaper,
        // which is fine for English and wrong for everything else.
        buffer.set_text(&spec.text, &Attrs::new().family(Family::SansSerif), Shaping::Advanced, None);
        buffer.shape_until_scroll(false);

        let mut glyphs = 0usize;
        let mut missing = 0usize;
        let mut width = 0f32;
        let mut lines = 0usize;
        // Where the last line we are allowed to draw runs past its box.
        let mut cut: Option<usize> = None;
        for run in buffer.layout_runs() {
            lines += 1;
            width = width.max(run.line_w);
            for glyph in run.glyphs {
                glyphs += 1;
                if glyph.glyph_id == 0 {
                    missing += 1;
                }
            }
            if lines == spec.lines as usize {
                cut = run.glyphs.last().map(|g| g.end);
            }
        }
        Laid { glyphs, missing, width, lines, cut }
    }

    /// Lay `spec` out and draw it, cutting it short if it does not fit.
    pub fn render(&mut self, spec: &Spec) -> Raster {
        let laid = self.lay_out(spec);
        let allowed = spec.lines as usize;
        if laid.lines <= allowed {
            return self.raster(&spec.text, spec, false);
        }

        // Too tall. Cut at the end of the last line we are allowed and put an
        // ellipsis there, then lay it out again — once, and only for the
        // strings that actually overflow.
        //
        // Cut on a byte offset the shaper gave us, which is a character
        // boundary in the *original string*. Dropping glyphs instead would cut
        // inside a cluster, and half of a composed character is not a
        // character.
        let cut = laid.cut.unwrap_or(spec.text.len()).min(spec.text.len());
        let head = spec.text[..cut].trim_end();
        let shortened = trim_to_fit(self, spec, head);
        self.raster(&shortened, spec, true)
    }

    /// Take characters off the end until the ellipsis fits too.
    ///
    /// The last line was full before the ellipsis was added to it, so adding
    /// one pushes it onto a line of its own — an ellipsis alone on the third
    /// line of a two-line title, which is worse than the overflow.
    fn raster(&mut self, text: &str, spec: &Spec, clipped: bool) -> Raster {
        let size = spec.size_px();
        let mut buffer = Buffer::new(&mut self.system, Metrics::new(size, size * 1.3));
        {
            let mut buffer = buffer.borrow_with(&mut self.system);
            buffer.set_wrap(if spec.width_px().is_some() { Wrap::WordOrGlyph } else { Wrap::None });
            buffer.set_size(spec.width_px(), None);
            buffer.set_text(text, &Attrs::new().family(Family::SansSerif), Shaping::Advanced, None);
            buffer.shape_until_scroll(false);
        }

        let mut width = 0f32;
        let mut lines = 0usize;
        for run in buffer.layout_runs() {
            width = width.max(run.line_w);
            lines += 1;
        }
        let w = spec.width_px().map(|w| w.min(width.ceil())).unwrap_or(width.ceil()).ceil().max(1.0);
        let h = (size * 1.3 * lines.max(1) as f32).ceil().max(1.0);
        let (w, h) = (w as u32, h as u32);
        let mut coverage = vec![0u8; (w as usize) * (h as usize)];

        // Baseline-relative, which is what the shaper works in.
        let runs: Vec<_> = buffer.layout_runs().map(|r| (r.line_y, r.glyphs.to_vec())).collect();
        for (line_y, glyphs) in runs {
            for glyph in glyphs {
                let physical = glyph.physical((0.0, 0.0), 1.0);
                let Some(image) = self.swash.get_image(&mut self.system, physical.cache_key).clone()
                else {
                    continue;
                };
                let image = &image;
                blit(
                    &mut coverage,
                    w,
                    h,
                    physical.x + image.placement.left,
                    physical.y + line_y as i32 - image.placement.top,
                    image,
                );
            }
        }
        Raster { width: w, height: h, coverage, clipped }
    }
}

/// Put the ellipsis on, shortening until it fits on the lines allowed.
fn trim_to_fit(fonts: &mut Fonts, spec: &Spec, head: &str) -> String {
    let mut head = head.to_owned();
    for _ in 0..64 {
        let candidate = format!("{head}{ELLIPSIS}");
        let probe = Spec { text: candidate.clone(), ..spec.clone() };
        if fonts.lay_out(&probe).lines <= spec.lines as usize {
            return candidate;
        }
        match shorten_once(&head) {
            Some(shorter) if !shorter.is_empty() => head = shorter,
            _ => break,
        }
    }
    format!("{head}{ELLIPSIS}")
}

/// One character off the end, and any space that character was hiding behind.
fn shorten_once(head: &str) -> Option<String> {
    let mut chars: Vec<char> = head.chars().collect();
    chars.pop()?;
    Some(chars.into_iter().collect::<String>().trim_end().to_owned())
}

struct Laid {
    glyphs: usize,
    /// Glyphs no installed face could draw. Zero, or the text is boxes.
    missing: usize,
    #[allow(dead_code)]
    width: f32,
    lines: usize,
    cut: Option<usize>,
}

/// One glyph's coverage into the page, clipped at every edge.
fn blit(page: &mut [u8], w: u32, h: u32, x: i32, y: i32, image: &cosmic_text::SwashImage) {
    let gw = image.placement.width as i32;
    let gh = image.placement.height as i32;
    for row in 0..gh {
        let py = y + row;
        if py < 0 || py >= h as i32 {
            continue;
        }
        for col in 0..gw {
            let px = x + col;
            if px < 0 || px >= w as i32 {
                continue;
            }
            let from = (row * gw + col) as usize;
            let Some(&value) = image.data.get(from) else { continue };
            if value == 0 {
                continue;
            }
            let to = py as usize * w as usize + px as usize;
            // Lighten rather than overwrite: glyphs overlap at their edges and
            // a plain copy leaves a seam through the join.
            page[to] = page[to].max(value);
        }
    }
}

/// Rendered text, kept as textures so a name is only shaped once.
///
/// Titles repeat: the same card is drawn every frame, and the same name comes
/// back every time the list is scrolled past. Shaping and rasterising are the
/// expensive half of this module and neither answer ever changes, so the work
/// is done once and the result is a texture.
///
/// The texture is white with the coverage as its alpha, tinted at draw time.
/// One raster then serves a name however it is coloured — dim in the metadata
/// line, bright under the cursor — instead of one per colour.
pub struct Painter<'a> {
    fonts: Fonts,
    creator: &'a TextureCreator<WindowContext>,
    cache: HashMap<Spec, Drawn<'a>>,
}

struct Drawn<'a> {
    texture: Texture<'a>,
    width: u32,
    height: u32,
}

/// Kept small on purpose while there is nothing to evict for. A real cursor
/// through 2,506 games wants a least-recently-used bound; this wants to not
/// grow without limit before that exists.
const CACHE_LIMIT: usize = 512;

impl<'a> Painter<'a> {
    pub fn new(creator: &'a TextureCreator<WindowContext>) -> Result<Self> {
        Ok(Painter { fonts: Fonts::load()?, creator, cache: HashMap::new() })
    }

    pub fn faces(&self) -> usize {
        self.fonts.faces()
    }

    /// How big this text comes out, in pixels, without drawing it.
    pub fn measure(&mut self, spec: &Spec) -> (u32, u32) {
        let drawn = self.entry(spec);
        (drawn.width, drawn.height)
    }

    /// Whether anything installed can draw this string.
    ///
    /// Worth asking at startup on a machine we did not build. A library with
    /// Japanese titles on a machine with no CJK face draws rows of empty
    /// boxes, and nothing about that looks like a missing font — it looks like
    /// the names are wrong.
    pub fn can_draw(&mut self, text: &str) -> bool {
        self.fonts.can_draw(text)
    }

    /// Lay a label out and say whether it had to be cut to fit.
    ///
    /// The full name is what a detail pane shows when the card's is cut, and
    /// knowing which those are is how the cut is judged while it is being
    /// tuned.
    pub fn is_clipped(&mut self, spec: &Spec) -> bool {
        self.fonts.render(spec).clipped
    }

    /// Draw it, with its top-left corner at `x`, `y` in pixels.
    pub fn draw(
        &mut self,
        canvas: &mut WindowCanvas,
        spec: &Spec,
        x: f32,
        y: f32,
        color: Color,
    ) {
        let (w, h) = {
            let drawn = self.entry(spec);
            (drawn.width, drawn.height)
        };
        let Some(drawn) = self.cache.get_mut(spec) else { return };
        drawn.texture.set_color_mod(color.r, color.g, color.b);
        drawn.texture.set_alpha_mod(color.a);
        let _ = canvas.copy(&drawn.texture, None, Rect::new(x as i32, y as i32, w, h));
    }

    fn entry(&mut self, spec: &Spec) -> &Drawn<'a> {
        if !self.cache.contains_key(spec) {
            if self.cache.len() >= CACHE_LIMIT {
                self.cache.clear();
            }
            let raster = self.fonts.render(spec);
            let drawn = self.upload(&raster);
            self.cache.insert(spec.clone(), drawn);
        }
        &self.cache[spec]
    }

    fn upload(&self, raster: &Raster) -> Drawn<'a> {
        let mut texture = self
            .creator
            .create_texture_static(PixelFormatEnum::ARGB8888, raster.width, raster.height)
            .expect("a texture the size of one label");
        // White, with the coverage as alpha. Tinted at draw time.
        let mut rgba = Vec::with_capacity(raster.coverage.len() * 4);
        for &value in &raster.coverage {
            rgba.extend_from_slice(&[255, 255, 255, value]);
        }
        let _ = texture.update(None, &rgba, raster.width as usize * 4);
        texture.set_blend_mode(BlendMode::Blend);
        Drawn { texture, width: raster.width, height: raster.height }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fonts() -> Fonts {
        Fonts::load().expect("a machine with no fonts cannot run this test")
    }

    #[test]
    fn a_machine_has_fonts_and_we_find_them() {
        assert!(fonts().faces() > 0);
    }

    /// The failure nobody reports: a Japanese title in a Latin face is a row
    /// of empty boxes, and it looks deliberate.
    #[test]
    fn japanese_finds_a_face_that_can_draw_it() {
        let mut f = fonts();
        assert!(f.can_draw("ゼルダの伝説"), "no CJK face, or fallback is not working");
        assert!(f.can_draw("スーパーマリオブラザーズ"));
    }

    #[test]
    fn so_does_the_alphabet_it_was_already_good_at() {
        let mut f = fonts();
        assert!(f.can_draw("Super Mario Bros."));
        assert!(f.can_draw("Pokémon Crystal"));
    }

    #[test]
    fn a_short_name_is_drawn_whole() {
        let mut f = fonts();
        let out = f.render(&Spec::new("Metroid", 14.0, 1.0).wrapped(200.0, 2));
        assert!(!out.clipped, "a name that fits was cut short");
        assert!(out.width > 0 && out.height > 0);
    }

    /// The rule this module exists for: a title too long for its card stops,
    /// and says that it stopped.
    #[test]
    fn a_long_name_is_cut_short_and_says_so() {
        let mut f = fonts();
        let long = "Mortal Kombat II: The Very Long Subtitle Nobody Asked For, Special Edition";
        let out = f.render(&Spec::new(long, 14.0, 1.0).wrapped(150.0, 2));
        assert!(out.clipped, "a name far too long was not cut");
        // And it stayed inside its box.
        assert!(out.width <= 151, "cut to {} points wide, box was 150", out.width);
    }

    /// CJK has no spaces, so "break at spaces" would never break at all and a
    /// Japanese title would run off the side of every card.
    #[test]
    fn japanese_wraps_even_though_it_has_no_spaces() {
        let mut f = fonts();
        let long = "ドラゴンクエストIII そして伝説へ".repeat(3);
        let out = f.render(&Spec::new(&long, 14.0, 1.0).wrapped(150.0, 2));
        assert!(out.width <= 151, "ran off the card: {} points", out.width);
        assert!(out.clipped);
    }

    /// Two lines is two lines. A title that spills onto a third is a card that
    /// overlaps the one below it.
    #[test]
    fn the_line_limit_is_a_limit() {
        let mut f = fonts();
        let long = "Castlevania: Symphony of the Night Deluxe Anniversary Collection";
        for lines in [1u16, 2, 3] {
            let out = f.render(&Spec::new(long, 14.0, 1.0).wrapped(150.0, lines));
            let at_most = (14.0 * 1.3 * lines as f32).ceil() as u32 + 1;
            assert!(out.height <= at_most, "{lines} lines came out {}px tall", out.height);
        }
    }

    /// Scale is part of what a rendered string *is*: the same title at the
    /// same point size is a different set of pixels on a different display.
    #[test]
    fn the_same_text_at_twice_the_scale_is_twice_the_size() {
        let mut f = fonts();
        let one = f.render(&Spec::new("Metroid", 14.0, 1.0));
        let two = f.render(&Spec::new("Metroid", 14.0, 2.0));
        assert!(two.height >= one.height * 2 - 2, "{} vs {}", two.height, one.height);
    }

    /// Empty is not a crash, and neither is a string of nothing but spaces.
    #[test]
    fn nothing_to_draw_draws_nothing() {
        let mut f = fonts();
        for text in ["", "   ", "\n"] {
            let out = f.render(&Spec::new(text, 14.0, 1.0).wrapped(150.0, 2));
            assert!(out.width >= 1 && out.height >= 1, "{text:?} produced no raster at all");
        }
    }

    /// Something actually landed on the page. A rasteriser that silently draws
    /// nothing passes every size assertion above.
    #[test]
    fn the_glyphs_reach_the_page() {
        let mut f = fonts();
        let out = f.render(&Spec::new("Metroid", 24.0, 1.0));
        let ink: usize = out.coverage.iter().filter(|&&v| v > 0).count();
        assert!(ink > 20, "only {ink} pixels of ink for a whole word");
    }
}
