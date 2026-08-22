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
use romm_desktop::layout::{Panes, Scale, Viewport};
use crate::gfx::{Gfx, Rgba};
use romm_desktop::{binds, padpoll, rowwindow};
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
    let mut art = covers::Covers::new(config.media_dir(), config.media.list_art.clone());

    // The library, straight out of the metadata cache the other front ends
    // read. Nothing is fetched and nothing is written.
    let mut lib = library::Library::open(std::path::Path::new("cache.sqlite3"), config.media_dir())
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
                    if let Some(at) = hits.at(x as f32, y as f32) {
                        lib.point_at(at);
                    }
                }
                Event::MouseButtonDown { mouse_btn, x, y, .. } => match mouse_btn {
                    sdl2::mouse::MouseButton::Left => {
                        if let Some(at) = hits.at(x as f32, y as f32) {
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
/// The colours, in one place. A palette rather than numbers scattered through
/// the drawing, because the next phase is the glass and it will want to talk
/// about these by name.
mod paint {
    use crate::gfx::Rgba;

    pub const BACKGROUND: Rgba = Rgba::rgb(18, 18, 22);
    pub const COLUMN: Rgba = Rgba(0.10, 0.10, 0.13, 0.55);
    pub const CARD: Rgba = Rgba(0.17, 0.17, 0.21, 0.75);
    pub const CURSOR: Rgba = Rgba::rgb(90, 130, 190);
    pub const TEXT: Rgba = Rgba::rgb(228, 230, 238);
    pub const DIM: Rgba = Rgba::rgb(140, 142, 152);
    pub const FAINT: Rgba = Rgba::rgb(96, 98, 108);
}

/// The sizes, in points. Nothing here is a pixel.
mod size {
    pub const CARD: f32 = 150.0;
    pub const GAP: f32 = 14.0;
    pub const PICKER: f32 = 260.0;
    pub const ASIDE: f32 = 320.0;
    /// How tall a card's artwork is, for a console whose covers are this
    /// shape. Not a constant: a PSP UMD case is 0.58 and a SNES box 1.37.
    pub fn art(aspect: f32) -> f32 {
        CARD / aspect.clamp(0.3, 3.0)
    }
    pub const LABEL: f32 = 13.0;
    /// Two lines of label, and the gap above them.
    pub const CAPTION: f32 = LABEL * 1.3 * 2.0 + 4.0;
    pub const ROW: f32 = 30.0;
    pub const TITLE: f32 = 15.0;
}

fn draw(
    gfx: &Gfx,
    frosted: Option<&glass::Glass>,
    painter: &mut text::Painter,
    art: &mut covers::Covers,
    lib: &mut library::Library,
    screen: &Viewport,
) -> Hits {
    let mut hits = Hits::default();
    let px = |points: f32| screen.scale.px(points);
    let panes = screen.panes();
    let showing_games = lib.view == library::View::Roms;

    // The picker column, where there is room for one. Where there is not, it
    // is the whole pane until a console is opened — which is the one-pane
    // arrangement, and the handheld's.
    let whole = (screen.width_px, screen.height_px);
    let pane = |x: f32, y: f32, w: f32, h: f32, tint| match frosted {
        Some(glass) => glass.panel(gfx, whole, x, y, w, h, tint),
        // No glass on this machine: a flat panel, the same shape and the same
        // colour, that simply does not show what is behind it.
        None => gfx.rect(x, y, w, h, tint),
    };

    let picker_column = panes >= Panes::Two;
    let mut left = 0.0;
    if picker_column {
        pane(0.0, 0.0, px(size::PICKER), screen.height_px, paint::COLUMN);
        left = size::PICKER + size::GAP;
    }
    let mut right = screen.width();
    if panes == Panes::Three {
        pane(
            px(screen.width() - size::ASIDE),
            0.0,
            px(size::ASIDE),
            screen.height_px,
            paint::COLUMN,
        );
        right = screen.width() - size::ASIDE - size::GAP;
        draw_detail(gfx, painter, art, lib, screen, screen.width() - size::ASIDE);
    }

    if lib.consoles.is_empty() {
        // A blank window is indistinguishable from a broken one. The library
        // being empty is a thing that happens — a fresh install, or a cache
        // that was never synced — and the app has to say which.
        let spec = text::Spec::new(
            "No consoles in this library.\nRun `romm-desktop sync` to fill it.",
            size::TITLE,
            screen.scale.factor(),
        )
        .wrapped(screen.width() - size::GAP * 4.0, 2);
        let (w, h) = painter.measure(gfx, &spec);
        painter.draw(
            gfx,
            &spec,
            (screen.width_px - w as f32) / 2.0,
            (screen.height_px - h as f32) / 2.0,
            paint::DIM,
        );
    } else if picker_column || !showing_games {
        let width = if picker_column { size::PICKER } else { screen.width() };
        draw_consoles(gfx, frosted, painter, lib, screen, &mut hits, width, !showing_games || !picker_column);
    }

    if showing_games {
        let middle = (right - left - size::GAP).max(0.0);
        let columns = (((middle + size::GAP) / (size::CARD + size::GAP)).floor() as usize).max(1);
        lib.relayout(columns);
        draw_games(gfx, frosted, painter, art, lib, screen, &mut hits, left, columns);
    }

    // What the window thinks it is. On screen rather than in the terminal
    // because what is worth watching is what happens while an edge is dragged.
    let filters = lib.filters();
    let readout = format!(
        "{:.0}x{:.0}pt · {:.2}x · {:?} · {} · {}{} · {} covers held",
        screen.width(),
        screen.height(),
        screen.scale.factor(),
        panes,
        lib.order_label(),
        if showing_games {
            format!("{} games", lib.shown())
        } else {
            format!("{} consoles", lib.consoles.len())
        },
        if filters.is_empty() { String::new() } else { format!(" · {}", filters.join("+")) },
        art.holding(),
    );
    // What the two buttons do. Obvious once you know, and invisible until
    // then — a console list you cannot get past looks like a broken app
    // rather than one waiting to be told which console.
    let hint = if showing_games { "Esc back · s sort · f filter" } else { "Enter opens a console" };
    let spec = text::Spec::new(hint, 11.0, screen.scale.factor());
    painter.draw(gfx, &spec, px(size::GAP), screen.height_px - px(size::GAP + 14.0), paint::FAINT);

    if let Some(row) = lib.selected() {
        let name = text::Spec::new(&row.name, 12.0, screen.scale.factor());
        painter.draw(gfx, &name, px(size::GAP), screen.height_px - px(size::GAP + 30.0), paint::DIM);
    }
    let spec = text::Spec::new(readout, 11.0, screen.scale.factor());
    let (w, h) = painter.measure(gfx, &spec);
    painter.draw(
        gfx,
        &spec,
        screen.width_px - w as f32 - px(size::GAP),
        screen.height_px - h as f32 - px(size::GAP),
        paint::FAINT,
    );

    hits
}

/// The consoles, as a column of names.
#[allow(clippy::too_many_arguments)]
fn draw_consoles(
    gfx: &Gfx,
    frosted: Option<&glass::Glass>,
    painter: &mut text::Painter,
    lib: &mut library::Library,
    screen: &Viewport,
    hits: &mut Hits,
    width: f32,
    focused: bool,
) {
    let px = |points: f32| screen.scale.px(points);
    // Scrolled to keep the cursor on screen. A console list is thirty-five
    // rows, so this is the whole of the windowing it needs.
    let fits = ((screen.height() - size::GAP * 2.0) / size::ROW).floor().max(1.0) as usize;
    let first = lib.console_at.saturating_sub(fits.saturating_sub(1));

    for (offset, console) in lib.consoles.iter().enumerate().skip(first).take(fits) {
        let y = size::GAP + (offset - first) as f32 * size::ROW;
        hits.add(0.0, px(y - 3.0), px(width), px(size::ROW), offset);
        let on = offset == lib.console_at;
        if on {
            let (hx, hy, hw, hh) = (
                px(size::GAP / 2.0),
                px(y - 3.0),
                px(width - size::GAP),
                px(size::ROW - 2.0),
            );
            match (focused, frosted) {
                // The cursor is a solid colour wherever it is: a highlight you
                // can see through is one you have to look for.
                (true, _) => gfx.rect(hx, hy, hw, hh, paint::CURSOR),
                (false, Some(glass)) => glass.panel(
                    gfx,
                    (screen.width_px, screen.height_px),
                    hx, hy, hw, hh,
                    paint::CARD,
                ),
                (false, None) => gfx.rect(hx, hy, hw, hh, paint::CARD),
            }
        }
        let spec = text::Spec::new(&console.name, size::TITLE, screen.scale.factor())
            .wrapped(width - size::GAP * 3.0 - 40.0, 1);
        painter.draw(gfx, &spec, px(size::GAP), px(y), if on { paint::TEXT } else { paint::DIM });

        let count = text::Spec::new(console.games.to_string(), 11.0, screen.scale.factor());
        let (cw, _) = painter.measure(gfx, &count);
        painter.draw(
            gfx,
            &count,
            px(width - size::GAP) - cw as f32,
            px(y + 3.0),
            if on { paint::TEXT } else { paint::FAINT },
        );
    }
}

/// One console's games, as covers with their names under them.
#[allow(clippy::too_many_arguments)]
fn draw_games(
    gfx: &Gfx,
    frosted: Option<&glass::Glass>,
    painter: &mut text::Painter,
    art: &mut covers::Covers,
    lib: &mut library::Library,
    screen: &Viewport,
    hits: &mut Hits,
    left: f32,
    columns: usize,
) {
    let px = |points: f32| screen.scale.px(points);
    // The artwork's own shape, so a console of tall boxes gets tall cards and
    // one of wide ones gets wide.
    let cover_height = size::art(lib.aspect);
    let step = cover_height + size::CAPTION + size::GAP;

    // Which rows to draw at all. `rowwindow` is the webview's own arithmetic,
    // ported into the core — on 2,506 arcade games this is the difference
    // between a band of a few dozen and every one of them.
    //
    // Scrolled to keep the cursor on screen rather than by a scrollbar, since
    // there is no pointer on a handheld. `scroll_to` answers with nothing when
    // it is already there, which is what stops the list moving under the
    // reader on every keypress.
    let viewport = (screen.height() - size::GAP).max(step);
    let (shown, at, was) = (lib.shown(), lib.at, lib.scrolled);
    let ask = |top: f32| rowwindow::Ask::new(shown, columns, step, top, viewport);
    let top = rowwindow::scroll_to(at, ask(was)).unwrap_or(was);
    lib.scrolled = top;
    let band = rowwindow::band(ask(top));

    let rows: Vec<_> = lib
        .showing()
        .enumerate()
        .skip(band.first)
        .take(band.count)
        .map(|(i, (r, stem))| (i, r.id, r.name.clone(), r.favourite, r.platform.clone(), stem.to_owned()))
        .collect();

    for (i, id, name, favourite, platform, stem) in rows {
        let (row, col) = (i / columns, i % columns);
        let x = left + size::GAP + col as f32 * (size::CARD + size::GAP);
        let y = size::GAP + row as f32 * step - top;
        if y + step < 0.0 || y > screen.height() {
            continue;
        }

        let on = i == lib.at;
        let frame = (px(x), px(y), px(size::CARD), px(cover_height));
        // The whole card, artwork and caption, is what a pointer is over.
        hits.add(frame.0, frame.1, frame.2, px(cover_height + size::CAPTION), i);
        match art.get(gfx, id, &platform, &stem) {
            // Fitted, not stretched. The slot is the console's shape and the
            // picture keeps its own, so a stray landscape screenshot among the
            // box art is letterboxed rather than squashed.
            Some(cover) => {
                gfx.image_fitted(cover, frame.0, frame.1, frame.2, frame.3, Rgba::WHITE)
            }
            // No artwork on this machine. A pane of glass rather than nothing,
            // so the grid keeps its shape and a game with no cover is still a
            // thing you can put the cursor on — and it looks like an empty
            // card rather than a hole.
            None => match frosted {
                Some(glass) => glass.panel(
                    gfx,
                    (screen.width_px, screen.height_px),
                    frame.0, frame.1, frame.2, frame.3,
                    paint::CARD,
                ),
                None => gfx.rect(frame.0, frame.1, frame.2, frame.3, paint::CARD),
            },
        }
        if on {
            outline(gfx, frame.0, frame.1, frame.2, frame.3, px(3.0), paint::CURSOR);
        }

        let spec = text::Spec::new(&name, size::LABEL, screen.scale.factor())
            .wrapped(size::CARD, 2);
        painter.draw(
            gfx,
            &spec,
            px(x),
            px(y + cover_height + 4.0),
            if on { paint::TEXT } else { paint::DIM },
        );
        if favourite {
            let star = text::Spec::new("★", 11.0, screen.scale.factor());
            painter.draw(gfx, &star, px(x + 4.0), px(y + 4.0), paint::TEXT);
        }
    }
}

/// Where each row of the list on screen was drawn, in pixels.
///
/// The mouse needs it and nothing else does: a pointer arrives at a place and
/// has to be told which row that is. Rebuilt every frame, because that is when
/// the answer is known.
#[derive(Default)]
pub struct Hits {
    spots: Vec<(f32, f32, f32, f32, usize)>,
}

impl Hits {
    fn add(&mut self, x: f32, y: f32, w: f32, h: f32, index: usize) {
        self.spots.push((x, y, w, h, index));
    }

    /// Which row is under this point, if any.
    fn at(&self, x: f32, y: f32) -> Option<usize> {
        self.spots
            .iter()
            .find(|(sx, sy, w, h, _)| x >= *sx && y >= *sy && x < sx + w && y < sy + h)
            .map(|(_, _, _, _, index)| *index)
    }
}

/// The preview column: the cover, the name, and what is known about it.
fn draw_detail(
    gfx: &Gfx,
    painter: &mut text::Painter,
    art: &mut covers::Covers,
    lib: &library::Library,
    screen: &Viewport,
    left: f32,
) {
    let px = |points: f32| screen.scale.px(points);
    let Some(detail) = lib.detail() else { return };
    let width = size::ASIDE - size::GAP * 2.0;
    let mut y = size::GAP;

    // The cover, as large as the column allows and in its own shape.
    let tall = size::art(lib.aspect);
    if let Some(cover) = art.get(gfx, detail.id, &detail.platform, &detail.stem) {
        let height = tall * (width / size::CARD);
        gfx.image_fitted(cover, px(left + size::GAP), px(y), px(width), px(height), Rgba::WHITE);
        y += height + size::GAP;
    }

    // The name, over as many lines as it takes. This is the one place a title
    // is not cut short — the card above had to fit it into 150 points and this
    // column is where the whole thing goes.
    let name = text::Spec::new(&detail.name, size::TITLE, screen.scale.factor())
        .wrapped(width, 4);
    let (_, name_height) = painter.measure(gfx, &name);
    painter.draw(gfx, &name, px(left + size::GAP), px(y), paint::TEXT);
    y += screen.scale.pt(name_height as f32) + size::GAP;

    if detail.favourite {
        let star = text::Spec::new("★ Starred", 11.0, screen.scale.factor());
        painter.draw(gfx, &star, px(left + size::GAP), px(y), paint::TEXT);
        y += 18.0;
    }

    // Label on the left, value on the right, which is the shape the webview's
    // pane uses and the one a list of facts wants.
    for (label, value) in detail.facts() {
        let l = text::Spec::new(label, 11.0, screen.scale.factor());
        let v = text::Spec::new(&value, 11.0, screen.scale.factor()).wrapped(width * 0.55, 2);
        painter.draw(gfx, &l, px(left + size::GAP), px(y), paint::FAINT);
        painter.draw(gfx, &v, px(left + size::GAP + width * 0.45), px(y), paint::DIM);
        y += 17.0;
        if y > screen.height() - size::GAP {
            break;
        }
    }
}

/// A border, as four filled edges. SDL draws a one-pixel rectangle and the
/// cursor needs to be visible against artwork.
fn outline(gfx: &Gfx, x: f32, y: f32, w: f32, h: f32, t: f32, color: Rgba) {
    gfx.rect(x, y, w, t, color);
    gfx.rect(x, y + h - t, w, t, color);
    gfx.rect(x, y, t, h, color);
    gfx.rect(x + w - t, y, t, h, color);
}

/// Unused for now, and kept honest: the tables this front end resolves through
/// are the same ones the desktop app writes.
#[allow(dead_code)]
fn actions() -> &'static [binds::Action] {
    binds::ACTIONS
}
