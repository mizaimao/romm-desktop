// The SDL front end: a window, a loop, and input.
//
// Phase one of docs/handheld-frontend.md task 3. There is nothing to look at
// yet beyond proof that the parts fit — what this establishes is the shape
// everything after it is written against:
//
//   * The window is **resizable**, and the layout answers. Not a fixed
//     backbuffer scaled up. On the handheld the size is settled once at
//     startup; on a desktop somebody drags an edge, and doing that is the
//     fastest way to find every place a pixel got hardcoded.
//   * Nothing is measured in pixels. A card is 150 *points*; `romm_desktop::
//     layout` turns points into pixels for whatever panel it lands on. The
//     moment a number in a view means pixels, that view has picked a screen.
//   * Input resolves through `romm_desktop::binds`, so a rebind made in the
//     desktop app is a rebind here. The pad's deadzones and repeat timings
//     come from `romm_desktop::padpoll`, which the webview only ever had a
//     copy of.
//
// It opens at 960x720 on purpose. That is the handheld, and living at 4:3
// every day is what stops the constrained case being a surprise at the end.

use anyhow::{Context, Result};
use romm_desktop::layout::{Edges, Panes, Rect, Scale, Size, Viewport};
use crate::gfx::{Gfx, Rgba};
use romm_desktop::{padpoll, rowwindow};
use sdl2::event::{Event, WindowEvent};
use std::collections::BTreeSet;

mod backdrop;
mod covers;
mod gfx;
mod glass;
mod input;
mod library;
mod text;

/// The handheld's panel, and this window's default.
const POCKET: (u32, u32) = (960, 720);

/// How far away a screen is assumed to be when nothing says otherwise: a desk.
///
/// The handheld says otherwise, in `[appearance] viewing_distance_cm`. It has
/// to: a 4" 960x720 panel is around 300 DPI, and at desk distance that is a
/// scale of three and a screen with room for two covers on it.
const DESK_CM: f32 = 60.0;

fn main() -> Result<()> {
    // Where the app's files are. Config, cache and library are all addressed
    // relative to one directory, and which one depends on how this was
    // started — see `romm_desktop::datadir`. Without it `cache.sqlite3` is
    // opened relative to whatever directory the binary was launched from, and
    // `Cache::open` *creates* the file, so instead of failing it makes an
    // empty database and shows a library of nothing.
    romm_desktop::datadir::anchor();

    // The renderer has to be the GL one, because the backdrop is a shader and
    // shaders need a context to be in. SDL picks Metal first on macOS and
    // would give us one we cannot draw into; the handheld has only GL anyway,
    // so this makes both machines the same rather than special-casing one.
    let sdl = sdl2::init().map_err(anyhow::Error::msg).context("starting SDL")?;
    let video = sdl.video().map_err(anyhow::Error::msg).context("opening the display")?;

    // Ask for the context the shader needs, *before* the window is made —
    // SDL fixes the attributes at creation and there is no changing them
    // after. Left alone, macOS hands back a legacy 2.1 context and the shader
    // fails with "version '330' is not supported", which is a true statement
    // about a context nobody asked for.
    //
    // Core 3.3 on a desktop and GLES 3.0 on the handheld: the two dialects the
    // backdrop is written in, and the only difference between them is the
    // version line.
    {
        let attr = video.gl_attr();
        if cfg!(any(target_os = "android", target_os = "linux")) {
            attr.set_context_profile(sdl2::video::GLProfile::GLES);
            attr.set_context_version(3, 0);
        } else {
            attr.set_context_profile(sdl2::video::GLProfile::Core);
            attr.set_context_version(3, 3);
        }
    }

    let window = open_window(&video).map_err(anyhow::Error::msg).context("opening a window")?;

    // The config, read once. Everything the front end needs from it is settled
    // before the first frame; nothing below re-reads a file.
    let config = romm_desktop::config::Config::load().unwrap_or_default();
    let held_at = config.appearance.viewing_distance_cm.unwrap_or(DESK_CM);

    // The context, ours. Everything below draws through it.
    let _context = window
        .gl_create_context()
        .map_err(anyhow::Error::msg)
        .context("creating an OpenGL context")?;
    window.gl_set_context_to_current().map_err(anyhow::Error::msg)?;
    let mut gfx = unsafe { Gfx::new(&video) }.context("setting up the renderer")?;
    println!(
        "gl: {} · {}",
        unsafe { gfx::reported(gl::VERSION) },
        unsafe { gfx::reported(gl::SHADING_LANGUAGE_VERSION) }
    );
    // Vertical sync, so a menu does not run the fan up.
    let _ = video.gl_set_swap_interval(sdl2::video::SwapInterval::VSync);

    let display = window.display_index().unwrap_or(0);
    let mut scale = scale_for(&video, display, held_at);
    let mut painter = text::Painter::new().context("finding fonts")?;
    check_fonts(&mut painter);

    // The shader backdrop, underneath everything. Built after the renderer, so
    // the context it compiles against is the one the renderer made current.
    //
    // Not fatal: a machine whose driver will not compile it still gets a
    // library, and the message says why rather than the window being black.
    // The frosted panels. Not fatal either: a driver that will not draw into a
    // texture still gets a library, with flat panels instead of glass.
    let mut frosted = match unsafe { glass::Glass::new(POCKET.0, POCKET.1) } {
        Ok(g) => Some(g),
        Err(e) => {
            eprintln!("no glass: {e:#}");
            None
        }
    };

    let backdrop = match unsafe { backdrop::Backdrop::build(&video, "blobs") } {
        Ok(b) => {
            println!("backdrop: {}", b.style_label());
            Some(b)
        }
        Err(e) => {
            eprintln!("no backdrop: {e:#}");
            None
        }
    };

    // Box art, from the same folder the other front ends fill. Nothing is
    // fetched: what is on disk is drawn and what is not is a flat card.
    let mut art = covers::Covers::new(
        config.media_dir(),
        config.media.list_art.clone(),
        (config.icons.style.clone(), config.icons.set.clone()),
    );

    // The library, straight out of the metadata cache the other front ends
    // read. Nothing is fetched and nothing is written.
    let mut lib = library::Library::open(
        std::path::Path::new("cache.sqlite3"),
        config.media_dir(),
        config.local_roms_dir(),
    )
        .context("opening the library")?;
    if lib.consoles.is_empty() {
        eprintln!(
            "warning: no consoles in {}/cache.sqlite3 — run `romm-desktop sync` to fill it",
            std::env::current_dir().unwrap_or_default().display()
        );
    }
    println!("{} consoles", lib.consoles.len());

    let mut screen = viewport(&window, scale);
    say_where_we_are(&screen);

    // The bindings the desktop app writes. Resolved once — the loop below runs
    // at the display's refresh rate, and this is a scan of two tables.
    let bindings = config.bindings.clone();
    let pad_map = bindings.pad_map();

    let controller = sdl.game_controller().map_err(anyhow::Error::msg)?;
    let mut pads = input::Pads::open_first(&controller);
    let mut repeat = padpoll::Repeat::default();

    let timer = sdl.timer().map_err(anyhow::Error::msg).context("starting the clock")?;
    let mut events = sdl.event_pump().map_err(anyhow::Error::msg)?;
    // What the pad is holding, so the drawing can show that input arrived.
    let mut held: BTreeSet<String> = BTreeSet::new();

    // Where each thing was drawn last frame, so the mouse knows what it is
    // over. Filled by the drawing and read by the next event — which is the
    // only order that works, since what is on screen is decided while drawing
    // it.
    let mut hits: Hits = Hits::default();

    'running: loop {
        let now = ticks(&timer);

        for event in events.poll_iter() {
            match event {
                Event::Quit { .. } => break 'running,
                Event::Window { win_event: WindowEvent::SizeChanged(..), .. }
                | Event::Window { win_event: WindowEvent::Moved(..), .. } => {
                    // Moved as well as resized: dragging a window between a
                    // retina display and a plain one changes the scale
                    // without changing the size in points.
                    let on = window.display_index().unwrap_or(0);
                    scale = scale_for(&video, on, held_at);
                    screen = viewport(&window, scale);
                    say_where_we_are(&screen);
                }
                // The mouse. ES-DE makes this hard and it is the complaint
                // Frank had about it; here it is the same cursor the pad
                // moves, pointed at instead of stepped to.
                Event::MouseMotion { x, y, .. } => {
                    let (x, y) = pointer(&window, x, y);
                    if let Some(at) = hits.at(x, y) {
                        lib.point_at(at);
                    }
                }
                Event::MouseButtonDown { mouse_btn, x, y, .. } => match mouse_btn {
                    sdl2::mouse::MouseButton::Left => {
                        let (x, y) = pointer(&window, x, y);
                        if let Some(mode) = hits.mode_at(x, y) {
                            lib.mode = mode;
                        } else if let Some(tab) = hits.tab_at(x, y) {
                            lib.section = tab;
                        } else if let Some(at) = hits.at(x, y) {
                            lib.point_at(at);
                            // A click opens what it is on, which is what a
                            // double-click does in the webview — but there is
                            // nothing else a click on a console could mean.
                            act(&mut lib, "activate");
                        }
                    }
                    // The right button goes back, the way it does in every
                    // file manager.
                    sdl2::mouse::MouseButton::Right => {
                        if lib.at_top() {
                            break 'running;
                        }
                        act(&mut lib, "back");
                    }
                    _ => {}
                },
                Event::MouseWheel { y, .. } => {
                    // A wheel notch is a row, not a pixel: the cursor is what
                    // scrolls here, and a list that moves without it leaves
                    // the selection off screen.
                    let step = if y > 0 { "up" } else { "down" };
                    for _ in 0..y.unsigned_abs().min(4) {
                        act(&mut lib, step);
                    }
                }
                Event::KeyDown { keycode: Some(key), repeat: false, .. } => {
                    // Escape is bound to Back, not to quit — it was hardcoded
                    // here in the first commit and never taken out, so going
                    // back from a console closed the app instead. The window
                    // closes the window; Cmd-Q and Alt-F4 do what they always
                    // do.
                    if let Some(action) = input::action_for_key(&bindings, key) {
                        // Back at the top level leaves, which is what Back
                        // means on a handheld with no window to close and no
                        // Cmd-Q to press. Nothing is lost by it: this browses.
                        if matches!(action, "back" | "back2") && lib.at_top() {
                            break 'running;
                        }
                        act(&mut lib, action);
                    }
                }
                _ => {}
            }
        }

        // The pad, read whole rather than as events: how far a stick is pushed
        // and how long a button has been down are states, not edges, and
        // `padpoll` is written against them.
        let pressed = pads.pressed(&pad_map);
        for action in &pressed {
            if repeat.fire(action, now) {
                if matches!(action.as_str(), "back" | "back2") && lib.at_top() {
                    break 'running;
                }
                act(&mut lib, action);
            }
        }
        repeat.release(&pressed);
        held.clone_from(&pressed);

        gfx.resize(screen.width_px, screen.height_px);

        // The backdrop is drawn twice, deliberately. Once small, into a
        // texture that is then blurred — which is what the panels sample —
        // and once at full size behind everything. Reading the finished frame
        // back would save the second draw and cannot be done before the frame
        // exists, which is the whole reason `backdrop-filter` is expensive in
        // a browser too.
        let seconds = now as f32 / 1000.0;
        if let (Some(glass), Some(backdrop)) = (&mut frosted, &backdrop) {
            unsafe {
                let _ = glass.resize(screen.width_px as u32, screen.height_px as u32);
                let (w, h) = glass.blurred_size();
                glass.capture(&mut gfx, |_| backdrop.draw(w as f32, h as f32, seconds));
            }
        }

        gfx.clear(paint::BACKGROUND);
        if let Some(backdrop) = &backdrop {
            unsafe { backdrop.draw(screen.width_px, screen.height_px, seconds) };
        }
        hits = draw(&gfx, frosted.as_ref(), &mut painter, &mut art, &mut lib, &screen);
        window.gl_swap_window();
    }
    Ok(())
}

/// Where the pointer is, in the pixels everything is drawn in.
///
/// SDL reports the mouse in *window* coordinates and we draw in *drawable*
/// ones, and on a retina display those differ by two — so a click near the
/// middle of the window lands a quarter of the way in, and one in the bottom
/// half lands nowhere at all. Not the layout's scale, which is about how big
/// things should look; this is about which pixels the platform means.
fn pointer(window: &sdl2::video::Window, x: i32, y: i32) -> (f32, f32) {
    let (dw, dh) = window.drawable_size();
    let (ww, wh) = window.size();
    let across = if ww > 0 { dw as f32 / ww as f32 } else { 1.0 };
    let down = if wh > 0 { dh as f32 / wh as f32 } else { 1.0 };
    (x as f32 * across, y as f32 * down)
}

fn open_window(video: &sdl2::VideoSubsystem) -> Result<sdl2::video::Window, String> {
    video
        .window("RomM", POCKET.0, POCKET.1)
        .position_centered()
        .resizable()
        // Retina and the like. Without this the window is described in points
        // by the platform and drawn at half the resolution it could be.
        .allow_highdpi()
        .opengl()
        .build()
        .map_err(|e| e.to_string())
}

/// Milliseconds since SDL started, which is what `padpoll` counts in.
///
/// The subsystem is held rather than asked for each frame: `Sdl::timer`
/// initialises it, and doing that sixty times a second for a number is work
/// for nothing — and if it ever failed, time would silently stop and the
/// backdrop would freeze with nothing to say why.
fn ticks(timer: &sdl2::TimerSubsystem) -> f64 {
    timer.ticks64() as f64
}

/// What the window is, in points.
///
/// The *drawable* size, not the window size: on a retina display those differ
/// by the backing scale, and the drawable is the one with pixels in it.
fn viewport(window: &sdl2::video::Window, scale: Scale) -> Viewport {
    let (w, h) = window.drawable_size();
    Viewport::new(w as f32, h as f32, scale)
}

/// How big to draw, for the display this window is on.
///
/// Two corrections, and they are different things. The platform's own backing
/// scale — retina — says how many pixels there are per point it reports.
/// `viewed_from` says how big a point should be for a panel held at that
/// distance, which is what stops a 300-DPI handheld getting a scale of three
/// and a screen with room for two covers on it.
fn scale_for(video: &sdl2::VideoSubsystem, display: i32, held_at_cm: f32) -> Scale {
    let dpi = video
        .display_dpi(display)
        .map(|(d, _, _)| d)
        // A display that will not say. One point is one pixel, which is what
        // every screen did before anybody had two of them.
        .unwrap_or(romm_desktop::layout::BASELINE_DPI);
    Scale::viewed_from(dpi, held_at_cm)
}

/// Say what the machine can and cannot draw, once, at startup.
///
/// Both halves matter on hardware we did not build. A machine with no CJK face
/// turns every Japanese title into a row of empty boxes, and that does not look
/// like a missing font — it looks like the names are wrong, which is a bug
/// report about the library. And a card width that cuts every name short is
/// worth knowing before squinting at a 4" screen to find out.
fn check_fonts(painter: &mut text::Painter) {
    println!("{} faces installed", painter.faces());
    for probe in ["Metroid", "ゼルダの伝説", "Pokémon"] {
        if !painter.can_draw(probe) {
            eprintln!("warning: no installed face can draw {probe:?} — it will be drawn as boxes");
        }
    }
    // Which face each of the shared scripts is handed to, and whether it is
    // the one asked for. Chinese, Japanese and Korean share code points whose
    // correct shapes differ; `romm_desktop::script` reads the title and names
    // the family, and this says whether the machine had it. A handheld with
    // only a pan-CJK fallback installed will say so here rather than quietly
    // drawing Japanese titles in Chinese forms.
    for (what, sample) in [
        ("Latin etc.", "Metroid"),
        ("Japanese", "ゼルダの伝説"),
        ("Chinese (S)", "塞尔达传说"),
        ("Chinese (T)", "薩爾達傳說"),
        ("Korean", "젤다의 전설"),
    ] {
        let asked = painter.family_for(sample).unwrap_or("(any)").to_owned();
        if let Some(face) = painter.face_for(sample) {
            let note = if face == asked || asked == "(any)" { "" } else { "  <- not what was asked for" };
            println!("  {what:<12} asked {asked:<20} got {face}{note}");
        }
    }
    // A title long enough that it must be cut, and one short enough that it
    // must not. Both, because an ellipsis that never appears and one that
    // always does are the same bug from opposite sides.
    let long = "Mortal Kombat II: The Very Long Subtitle Nobody Asked For, Special Edition";
    let at_card = |t: &str| text::Spec::new(t, size::LABEL, 1.0).wrapped(size::CARD, 2);
    if !painter.is_clipped(&at_card(long)) {
        eprintln!("warning: a title far too long for a card was not cut short");
    }
    if painter.is_clipped(&at_card("Metroid")) {
        eprintln!("warning: a short title was cut short");
    }
}

fn say_where_we_are(screen: &Viewport) {
    println!(
        "{:.0}x{:.0}px at {:.2}x -> {:.0}x{:.0}pt, {:.2}:1, {:?}",
        screen.width_px,
        screen.height_px,
        screen.scale.factor(),
        screen.width(),
        screen.height(),
        screen.aspect(),
        screen.panes(),
    );
}

/// Hand an action to the library, and say if anything came of it.
///
/// A binding that resolves to nothing is printed rather than swallowed: an
/// action nobody handles looks exactly like a button that is not working.
fn act(lib: &mut library::Library, action: &str) {
    match lib.act(action) {
        Ok(true) => {}
        Ok(false) => println!("nothing does {action} yet"),
        Err(e) => eprintln!("{action}: {e:#}"),
    }
}

/// A grid of cards, in points, and the columns the window has room for.
///
/// Not a design. It is the smallest thing that is wrong on screen if the units
/// are wrong: cards that change physical size when the window is dragged
/// between displays, or a column that appears at the wrong width.
/// The colours, in one place.
mod paint {
    use crate::gfx::Rgba;

    pub const BACKGROUND: Rgba = Rgba::rgb(14, 15, 20);
    /// Furniture: darker than a panel, so it does not compete with artwork.
    pub const BAR: Rgba = Rgba(0.05, 0.05, 0.08, 0.72);
    pub const COLUMN: Rgba = Rgba(0.08, 0.09, 0.13, 0.55);
    pub const CARD: Rgba = Rgba(0.14, 0.15, 0.21, 0.62);
    pub const CURSOR: Rgba = Rgba::rgb(96, 140, 210);
    pub const TEXT: Rgba = Rgba::rgb(232, 234, 242);
    pub const DIM: Rgba = Rgba::rgb(150, 154, 168);
    pub const FAINT: Rgba = Rgba::rgb(104, 108, 124);
    /// The plate a mark sits on, so it reads against artwork of any colour.
    pub const MARK: Rgba = Rgba(0.0, 0.0, 0.0, 0.45);
    pub const STAR: Rgba = Rgba::rgb(240, 200, 90);
    /// On this machine, and on the server.
    pub const HERE: Rgba = Rgba::rgb(120, 200, 140);
    pub const AWAY: Rgba = Rgba::rgb(150, 160, 180);
}

/// The sizes, in points. Nothing here is a pixel.
mod size {
    use romm_desktop::layout::Edges;

    pub const GAP: f32 = 14.0;
    /// The tab row and the header under it, as `ui/style.css` has them.
    pub const TABS: f32 = 42.0;
    pub const HEADER: f32 = 38.0;
    pub const PICKER: f32 = 260.0;
    pub const ASIDE: f32 = 320.0;
    pub const CARD: f32 = 150.0;
    pub const LABEL: f32 = 13.0;
    pub const TITLE: f32 = 15.0;
    /// Two lines of label under a cover, and the marks over it.
    pub const CAPTION: f32 = LABEL * 1.3 * 2.0 + 6.0;
    pub const ROW: f32 = 30.0;
    /// A console tile: the machine's picture above, name and count below.
    pub const TILE: f32 = 190.0;
    pub const TILE_ART: f32 = 104.0;
    pub const TILE_CAPTION: f32 = 50.0;
    pub const ROUND: f32 = 10.0;
    /// The Sofa/Desk control in the tab row.
    pub const SWITCH: f32 = 130.0;
    /// The Continue playing strip: a heading, a row of covers, their names.
    pub const STRIP: f32 = 210.0;
    pub const STRIP_ART: f32 = 130.0;
    pub const ROUND_SMALL: f32 = 6.0;
    pub const PAD: Edges = Edges::all(GAP);

    /// How tall a card's artwork is for covers of this shape.
    pub fn art(aspect: f32) -> f32 {
        CARD / aspect.clamp(0.3, 3.0)
    }
}

/// Where each thing was drawn, so the mouse knows what it is over.
///
/// Rebuilt every frame, because that is when the answer is known, and in
/// pixels because that is what the pointer arrives in.
#[derive(Default)]
pub struct Hits {
    rows: Vec<(Rect, usize)>,
    tabs: Vec<(Rect, usize)>,
    modes: Vec<(Rect, library::Mode)>,
}

impl Hits {
    fn row(&mut self, at: Rect, index: usize) {
        self.rows.push((at, index));
    }

    fn tab(&mut self, at: Rect, index: usize) {
        self.tabs.push((at, index));
    }

    fn mode(&mut self, at: Rect, mode: library::Mode) {
        self.modes.push((at, mode));
    }

    fn mode_at(&self, x: f32, y: f32) -> Option<library::Mode> {
        self.modes.iter().find(|(r, _)| r.contains(x, y)).map(|(_, m)| *m)
    }

    fn at(&self, x: f32, y: f32) -> Option<usize> {
        self.rows.iter().find(|(r, _)| r.contains(x, y)).map(|(_, i)| *i)
    }

    /// Checked before the rows, because the tab row is drawn over them.
    fn tab_at(&self, x: f32, y: f32) -> Option<usize> {
        self.tabs.iter().find(|(r, _)| r.contains(x, y)).map(|(_, i)| *i)
    }
}

/// Everything one frame needs to draw itself, so no function takes eight
/// arguments and no call site gets two of them the wrong way round.
struct Frame<'a> {
    gfx: &'a Gfx,
    glass: Option<&'a glass::Glass>,
    painter: &'a mut text::Painter,
    art: &'a mut covers::Covers,
    screen: &'a Viewport,
    hits: Hits,
}

impl Frame<'_> {
    /// Points to pixels. The one conversion in the file.
    fn px(&self, r: Rect) -> Rect {
        let s = self.screen.scale.factor();
        Rect::new(r.x * s, r.y * s, r.w * s, r.h * s)
    }

    fn whole(&self) -> (f32, f32) {
        (self.screen.width_px, self.screen.height_px)
    }

    /// A pane of frosted glass, or a flat one where the machine has no glass.
    fn pane(&self, at: Rect, tint: Rgba, round: f32) {
        let at = self.px(at);
        let round = round * self.screen.scale.factor();
        let whole = self.whole();
        self.gfx.rounded(round, || match self.glass {
            Some(glass) => glass.panel(self.gfx, whole, at.x, at.y, at.w, at.h, tint),
            None => self.gfx.fill(at, tint),
        });
    }

    fn fill(&self, at: Rect, colour: Rgba, round: f32) {
        let at = self.px(at);
        let gfx = self.gfx;
        gfx.rounded(round * self.screen.scale.factor(), || gfx.fill(at, colour));
    }

    fn outline(&self, at: Rect, thickness: f32, colour: Rgba, round: f32) {
        let at = self.px(at);
        let s = self.screen.scale.factor();
        let gfx = self.gfx;
        gfx.rounded(round * s, || gfx.outline(at, thickness * s, colour));
    }

    fn spec(&self, text: impl Into<String>, size: f32) -> text::Spec {
        text::Spec::new(text, size, self.screen.scale.factor())
    }

    fn wrapped(&self, text: impl Into<String>, size: f32, width: f32, lines: u16) -> text::Spec {
        self.spec(text, size).wrapped(width, lines)
    }

    /// A label at the top left of a box. Returns its height, in points.
    fn label(&mut self, spec: &text::Spec, at: Rect, colour: Rgba) -> f32 {
        let at = self.px(at);
        let h = self.painter.put(self.gfx, spec, at, colour);
        self.screen.scale.pt(h)
    }

    fn label_right(&mut self, spec: &text::Spec, at: Rect, colour: Rgba) {
        let at = self.px(at);
        self.painter.put_right(self.gfx, spec, at, colour);
    }

    fn label_centred(&mut self, spec: &text::Spec, at: Rect, colour: Rgba) {
        let at = self.px(at);
        self.painter.put_centred(self.gfx, spec, at, colour);
    }

    /// A console's own picture, in a box.
    fn console_art(&mut self, slug: &str, at: Rect, round: f32) {
        let at = self.px(at);
        let round = round * self.screen.scale.factor();
        let gfx = self.gfx;
        if let Some(picture) = self.art.console(gfx, slug) {
            gfx.rounded(round, || gfx.picture(picture, at, Rgba::WHITE));
        }
    }

    /// A game's cover, in a box. Says whether there was one, so the caller can
    /// put a pane of glass there instead.
    fn cover(&mut self, id: i64, platform: &str, stem: &str, at: Rect, round: f32) -> bool {
        let at = self.px(at);
        let round = round * self.screen.scale.factor();
        let gfx = self.gfx;
        match self.art.get(gfx, id, platform, stem) {
            Some(cover) => {
                gfx.rounded(round, || gfx.picture(cover, at, Rgba::WHITE));
                true
            }
            None => false,
        }
    }
}

/// One frame.
///
/// The page divides itself: a tab row, a header, and a body split into as many
/// columns as the window can hold. Nothing below adds a gap to an offset.
fn draw(
    gfx: &Gfx,
    frosted: Option<&glass::Glass>,
    painter: &mut text::Painter,
    art: &mut covers::Covers,
    lib: &mut library::Library,
    screen: &Viewport,
) -> Hits {
    let mut f = Frame { gfx, glass: frosted, painter, art, screen, hits: Hits::default() };
    let page = Rect::new(0.0, 0.0, screen.width(), screen.height());

    let [tabs, header, body] = split3(page.column(
        0.0,
        &[Size::Fixed(size::TABS), Size::Fixed(size::HEADER), Size::Grow(1.0)],
    ));

    // How many columns the body gets, and which. The picker is only a column
    // of its own where there is room; below that it is the whole body until a
    // console is opened, which is the handheld's arrangement.
    // The window's width is a ceiling and Sofa/Desk is what is wanted under
    // it — see `layout::Panes::at_most`. A narrow window asking for columns
    // still gets one pane, and the handheld never gets three.
    let panes = screen.panes().at_most(lib.mode.panes());
    let showing_games = lib.view == library::View::Roms;
    let wants = match (panes, showing_games) {
        (Panes::Three, _) => vec![
            Size::Fixed(size::PICKER),
            Size::Grow(1.0),
            Size::Fixed(size::ASIDE),
        ],
        (Panes::Two, _) => vec![Size::Fixed(size::PICKER), Size::Grow(1.0)],
        (Panes::One, _) => vec![Size::Grow(1.0)],
    };
    let columns = body.row(size::GAP, &wants);
    let (picker, games, aside) = match panes {
        Panes::Three => (Some(columns[0]), columns[1], Some(columns[2])),
        Panes::Two => (Some(columns[0]), columns[1], None),
        Panes::One if showing_games => (None, columns[0], None),
        Panes::One => (Some(columns[0]), columns[0], None),
    };

    if let Some(picker) = picker {
        f.pane(picker, paint::COLUMN, size::ROUND);
    }
    if let Some(aside) = aside {
        f.pane(aside, paint::COLUMN, size::ROUND);
        draw_detail(&mut f, lib, aside.inset(size::PAD));
    }

    if lib.consoles.is_empty() {
        let spec = f.wrapped(
            "No consoles in this library.\nRun `romm-desktop sync` to fill it.",
            size::TITLE,
            page.w * 0.6,
            2,
        );
        f.label_centred(&spec, page, paint::DIM);
    } else if let Some(picker) = picker {
        // The strip of recently played games, above the consoles and only
        // where the column is wide enough to be a grid — a 260-point picker
        // has no room for a row of covers, and the webview does not draw one
        // there either.
        let area = picker.inset(size::PAD);
        let area = if picker.w >= size::TILE * 2.0 + size::GAP && !lib.recent().is_empty() {
            let (strip, rest) = area.split_top(size::STRIP);
            draw_recent(&mut f, lib, strip);
            rest.inset(Edges { top: size::GAP, ..Edges::default() })
        } else {
            area
        };
        draw_consoles(&mut f, lib, area, !showing_games);
    } else if !showing_games {
        draw_consoles(&mut f, lib, games.inset(size::PAD), true);
    }

    if showing_games {
        draw_games(&mut f, lib, games.inset(size::PAD));
    }

    draw_chrome(&mut f, lib, tabs, header);
    f.hits
}

fn split3(mut boxes: Vec<Rect>) -> [Rect; 3] {
    boxes.resize(3, Rect::new(0.0, 0.0, 0.0, 0.0));
    [boxes[0], boxes[1], boxes[2]]
}

/// The tab row and the header, across the top.
fn draw_chrome(f: &mut Frame, lib: &library::Library, tabs: Rect, header: Rect) {
    f.pane(Rect::new(tabs.x, tabs.y, tabs.w, tabs.h + header.h), paint::BAR, 0.0);

    // Tabs, each as wide as its own name — "Library" should not get the same
    // room as "My collections".
    let mut x = tabs.x + size::GAP;
    for (i, section) in library::SECTIONS.iter().enumerate() {
        let spec = f.spec(section.label, 13.0);
        let (w, _) = f.painter.measure(f.gfx, &spec);
        let width = f.screen.scale.pt(w as f32) + size::GAP * 1.5;
        let slot = Rect::new(x, tabs.y, width, tabs.h);
        let on = i == lib.section;
        if on {
            f.fill(Rect::new(slot.x, slot.bottom() - 3.0, slot.w, 3.0), paint::CURSOR, 2.0);
        }
        f.label_centred(
            &spec,
            slot,
            // A tab that does nothing yet is dimmer rather than missing.
            if on {
                paint::TEXT
            } else if section.ready {
                paint::DIM
            } else {
                paint::FAINT
            },
        );
        f.hits.tab(f.px(slot), i);
        x += width;
    }

    // Sofa or Desk, as a segmented control — the one thing in the header that
    // changes the shape of the whole window, so it sits in the tab row where
    // it is always reachable rather than inside a screen.
    let switch = Rect::new(tabs.right() - size::GAP - size::SWITCH, tabs.y + 7.0, size::SWITCH, tabs.h - 14.0);
    f.fill(switch, paint::MARK, size::ROUND_SMALL);
    let halves = switch.row(0.0, &[Size::Grow(1.0), Size::Grow(1.0)]);
    for (mode, half) in library::MODES.iter().zip(halves) {
        let on = *mode == lib.mode;
        if on {
            f.fill(half, paint::CURSOR, size::ROUND_SMALL);
        }
        let spec = f.spec(mode.label(), 12.0);
        f.label_centred(&spec, half, if on { paint::TEXT } else { paint::DIM });
        f.hits.mode(f.px(half), *mode);
    }

    // Where you are on the left, how the list is arranged on the right.
    let inner = header.inset(Edges::xy(size::GAP, 0.0));
    let here = match lib.view {
        library::View::Platforms => format!("{} consoles", lib.consoles.len()),
        library::View::Roms => match lib.console() {
            Some(c) => format!("{} — {} games", c.name, lib.shown()),
            None => String::new(),
        },
    };
    let title = f.spec(here, size::TITLE);
    let (_, th) = f.painter.measure(f.gfx, &title);
    let centred = Rect::new(inner.x, inner.y, inner.w, inner.h)
        .centre(inner.w, f.screen.scale.pt(th as f32));
    f.label(&title, Rect { x: inner.x, ..centred }, paint::TEXT);

    if lib.view == library::View::Roms {
        let filters = lib.filters();
        let arranged = if filters.is_empty() {
            lib.order_label().to_owned()
        } else {
            format!("{}  ·  {}", lib.order_label(), filters.join(" + "))
        };
        let spec = f.spec(arranged, 12.0);
        f.label_right(&spec, Rect { x: inner.x, ..centred }, paint::DIM);
    }
}

/// Continue playing: one row that runs off the side.
///
/// A shortcut rather than a second library — the webview stops at twenty for
/// the same reason, and past that it is a screen above the screen you wanted.
fn draw_recent(f: &mut Frame, lib: &library::Library, area: Rect) {
    let (heading, row) = area.split_top(22.0);
    let spec = f.spec("CONTINUE PLAYING", 11.0);
    f.label(&spec, heading, paint::FAINT);

    // As many as fit, and no wrapping: this is one row by definition.
    let card = size::CARD * 0.8;
    let across = row.fits(size::GAP, card);
    let grid = row.tracks(size::GAP, across);
    for (i, game) in lib.recent().iter().take(across).enumerate() {
        let cell = grid.cell(i, row.h);
        let (art, caption) = cell.split_top(size::STRIP_ART);
        if !f.cover(game.id, &game.platform, &game.stem, art, size::ROUND_SMALL) {
            f.pane(art, paint::CARD, size::ROUND_SMALL);
        }
        let name = f.wrapped(&game.name, 12.0, caption.w, 1);
        let used = f.label(&name, caption.inset(Edges { top: 4.0, ..Edges::default() }), paint::DIM);
        // The console under the name, with the mark that says whether it will
        // play offline — the strip mixes consoles, so the row cannot say which
        // one it is on.
        let under = Rect { y: caption.y + used + 6.0, h: 14.0, ..caption };
        let [dot, where_] =
            <[Rect; 2]>::try_from(under.row(4.0, &[Size::Fixed(9.0), Size::Grow(1.0)])).unwrap();
        let spec = f.spec(if game.downloaded { "●" } else { "○" }, 9.0);
        f.label(&spec, dot, if game.downloaded { paint::HERE } else { paint::AWAY });
        let spec = f.spec(&game.platform, 10.0);
        f.label(&spec, where_, paint::FAINT);
    }
}

/// The consoles: a grid of tiles where there is room, a list where there is
/// not — the same two shapes the webview switches between.
fn draw_consoles(f: &mut Frame, lib: &mut library::Library, area: Rect, focused: bool) {
    if area.w < size::TILE * 2.0 + size::GAP {
        let fits = (area.h / size::ROW).floor().max(1.0) as usize;
        let first = lib.console_at.saturating_sub(fits.saturating_sub(1));
        for (offset, console) in lib.consoles.iter().enumerate().skip(first).take(fits) {
            let slot = Rect::new(
                area.x,
                area.y + (offset - first) as f32 * size::ROW,
                area.w,
                size::ROW,
            );
            f.hits.row(f.px(slot), offset);
            let on = offset == lib.console_at;
            if on {
                match focused {
                    true => f.fill(slot, paint::CURSOR, size::ROUND_SMALL),
                    false => f.pane(slot, paint::CARD, size::ROUND_SMALL),
                }
            }
            let [name, count] =
                <[Rect; 2]>::try_from(slot.inset(Edges::xy(8.0, 5.0)).row(
                    6.0,
                    &[Size::Grow(1.0), Size::Fixed(46.0)],
                ))
                .unwrap();
            let spec = f.wrapped(&console.name, size::TITLE, name.w, 1);
            f.label(&spec, name, if on { paint::TEXT } else { paint::DIM });
            let spec = f.spec(console.games.to_string(), 11.0);
            f.label_right(&spec, count, if on { paint::TEXT } else { paint::FAINT });
        }
        return;
    }

    let across = area.fits(size::GAP, size::TILE);
    lib.relayout(across);
    let grid = area.tracks(size::GAP, across);
    let tile_h = size::TILE_ART + size::TILE_CAPTION;
    let rows = (area.h / (tile_h + size::GAP)).ceil().max(1.0) as usize;
    let first_row = (lib.console_at / across).saturating_sub(rows.saturating_sub(2));

    for (offset, console) in lib.consoles.iter().enumerate() {
        let row = offset / across;
        if row < first_row || row >= first_row + rows {
            continue;
        }
        let tile = grid.cell(offset - first_row * across, tile_h);
        f.hits.row(f.px(tile), offset);
        let on = offset == lib.console_at;

        f.pane(tile, paint::CARD, size::ROUND);
        if on {
            f.outline(tile, 2.0, paint::CURSOR, size::ROUND);
        }

        let inner = tile.inset(Edges::all(10.0));
        let (art, caption) = inner.split_top(size::TILE_ART - 10.0);
        f.console_art(&console.slug, art, 0.0);

        let name = f.wrapped(&console.name, 13.0, caption.w, 2);
        let used = f.label(&name, caption, if on { paint::TEXT } else { paint::DIM });
        let under = Rect { y: caption.y + used + 4.0, h: 14.0, ..caption };
        let [dot, count] =
            <[Rect; 2]>::try_from(under.row(4.0, &[Size::Fixed(9.0), Size::Grow(1.0)])).unwrap();
        let spec = f.spec("●", 9.0);
        f.label(&spec, dot, paint::HERE);
        let spec = f.spec(format!("{} games", console.games), 11.0);
        f.label(&spec, count, paint::FAINT);
    }
}

/// One console's games, as a wall of covers.
fn draw_games(f: &mut Frame, lib: &mut library::Library, area: Rect) {
    let cover_h = size::art(lib.aspect);
    let step = cover_h + size::CAPTION;
    let across = area.fits(size::GAP, size::CARD);
    lib.relayout(across);
    let grid = area.tracks(size::GAP, across);

    // Which rows to draw, and where the list is scrolled to keep the cursor
    // on screen. `rowwindow` answers both, and answers nothing when the
    // cursor is already visible — which is what stops the wall moving under
    // the reader on every keypress.
    let (shown, at, was) = (lib.shown(), lib.at, lib.scrolled);
    let ask = |top: f32| rowwindow::Ask::new(shown, across, step + size::GAP, top, area.h);
    let top = rowwindow::scroll_to(at, ask(was)).unwrap_or(was);
    lib.scrolled = top;
    let band = rowwindow::band(ask(top));

    let rows: Vec<_> = lib
        .showing()
        .enumerate()
        .skip(band.first)
        .take(band.count)
        .map(|(i, (r, stem))| {
            (i, r.id, r.name.clone(), r.favourite, r.downloaded, r.platform.clone(), stem.to_owned())
        })
        .collect();

    for (i, id, name, favourite, downloaded, platform, stem) in rows {
        let cell = grid.cell(i, step);
        let card = Rect { y: cell.y - top, ..cell };
        if card.bottom() < area.y || card.y > area.bottom() {
            continue;
        }
        f.hits.row(f.px(card), i);
        let (art, caption) = card.split_top(cover_h);
        let on = i == lib.at;

        // A pane of glass where there is no artwork, rather than a hole, so
        // the wall keeps its shape.
        if !f.cover(id, &platform, &stem, art, size::ROUND_SMALL) {
            f.pane(art, paint::CARD, size::ROUND_SMALL);
        }
        if on {
            f.outline(art, 3.0, paint::CURSOR, size::ROUND_SMALL);
        }

        // The two marks: starred, and whether it will play with the server
        // off. Both states are drawn, because no mark is not an answer.
        let marks = Rect::new(art.x + 5.0, art.y + 5.0, art.w, 16.0);
        let mut mark_x = marks.x;
        for (glyph, colour) in [
            favourite.then_some(("★", paint::STAR)),
            Some(if downloaded { ("●", paint::HERE) } else { ("○", paint::AWAY) }),
        ]
        .into_iter()
        .flatten()
        {
            let spec = f.spec(glyph, 12.0);
            let (w, _) = f.painter.measure(f.gfx, &spec);
            let w = f.screen.scale.pt(w as f32);
            let plate = Rect::new(mark_x - 3.0, marks.y, w + 6.0, marks.h);
            f.fill(plate, paint::MARK, 4.0);
            f.label(&spec, Rect { x: mark_x, y: marks.y + 1.0, ..plate }, colour);
            mark_x += w + 9.0;
        }

        let spec = f.wrapped(&name, size::LABEL, caption.w, 2);
        f.label(
            &spec,
            caption.inset(Edges { top: 4.0, ..Edges::default() }),
            if on { paint::TEXT } else { paint::DIM },
        );
    }
}

/// The preview column: the cover, the name, and what is known about it.
fn draw_detail(f: &mut Frame, lib: &library::Library, area: Rect) {
    let Some(detail) = lib.detail() else { return };
    let (art, rest) = area.split_top(area.w / lib.aspect.clamp(0.3, 3.0));
    // The game's own cover if there is one, and the console's picture if not —
    // an empty pane at the top of the column reads as a broken panel.
    if !f.cover(detail.id, &detail.platform, &detail.stem, art, size::ROUND_SMALL) {
        f.console_art(&detail.platform, art, size::ROUND_SMALL);
    }

    // The one place a title is not cut short: the card had to fit it into 150
    // points and this column is where the whole thing goes.
    let below = rest.inset(Edges { top: size::GAP, ..Edges::default() });
    let name = f.wrapped(&detail.name, size::TITLE, below.w, 4);
    let used = f.label(&name, below, paint::TEXT);
    let mut y = below.y + used + size::GAP;

    for (label, value) in detail.facts() {
        let line = Rect::new(below.x, y, below.w, 16.0);
        if line.bottom() > area.bottom() {
            break;
        }
        let [left, right] =
            <[Rect; 2]>::try_from(line.row(6.0, &[Size::Grow(4.0), Size::Grow(6.0)])).unwrap();
        let spec = f.spec(label, 11.0);
        f.label(&spec, left, paint::FAINT);
        let spec = f.wrapped(value, 11.0, right.w, 1);
        f.label(&spec, right, paint::DIM);
        y += 17.0;
    }
}
