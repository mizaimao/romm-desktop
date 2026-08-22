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
use romm_desktop::{binds, padpoll};
use sdl2::event::{Event, WindowEvent};
use sdl2::keyboard::Keycode;
use sdl2::rect::Rect;
use std::collections::BTreeSet;

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
    let sdl = sdl2::init().map_err(anyhow::Error::msg).context("starting SDL")?;
    let video = sdl.video().map_err(anyhow::Error::msg).context("opening the display")?;

    let window = video
        .window("RomM", POCKET.0, POCKET.1)
        .position_centered()
        .resizable()
        // Retina and the like. Without this the window is described in points
        // by the platform and drawn at half the resolution it could be.
        .allow_highdpi()
        .build()
        .context("opening a window")?;

    // The config, read once. Everything the front end needs from it is settled
    // before the first frame; nothing below re-reads a file.
    let config = romm_desktop::config::Config::load().unwrap_or_default();
    let held_at = config.appearance.viewing_distance_cm.unwrap_or(DESK_CM);

    let display = window.display_index().unwrap_or(0);
    let mut scale = scale_for(&video, display, held_at);
    let mut canvas = window.into_canvas().accelerated().build().context("getting a renderer")?;

    // The texture creator outlives everything drawn from it, which is what
    // lets rendered labels be kept as textures rather than rebuilt per frame.
    let creator = canvas.texture_creator();
    let mut painter = text::Painter::new(&creator).context("finding fonts")?;
    check_fonts(&mut painter);

    // The library, straight out of the metadata cache the other front ends
    // read. Nothing is fetched and nothing is written.
    let mut lib = library::Library::open(std::path::Path::new("cache.sqlite3"))
        .context("opening the library")?;
    println!("{} consoles", lib.consoles.len());

    let mut screen = viewport(&canvas, scale);
    say_where_we_are(&screen);

    // The bindings the desktop app writes. Resolved once — the loop below runs
    // at the display's refresh rate, and this is a scan of two tables.
    let bindings = config.bindings.clone();
    let pad_map = bindings.pad_map();

    let controller = sdl.game_controller().map_err(anyhow::Error::msg)?;
    let mut pads = input::Pads::open_first(&controller);
    let mut repeat = padpoll::Repeat::default();

    let mut events = sdl.event_pump().map_err(anyhow::Error::msg)?;
    // What the pad is holding, so the drawing can show that input arrived.
    let mut held: BTreeSet<String> = BTreeSet::new();

    'running: loop {
        let now = ticks(&sdl);

        for event in events.poll_iter() {
            match event {
                Event::Quit { .. } => break 'running,
                Event::Window { win_event: WindowEvent::SizeChanged(..), .. }
                | Event::Window { win_event: WindowEvent::Moved(..), .. } => {
                    // Moved as well as resized: dragging a window between a
                    // retina display and a plain one changes the scale
                    // without changing the size in points.
                    let on = canvas.window().display_index().unwrap_or(0);
                    scale = scale_for(&video, on, held_at);
                    screen = viewport(&canvas, scale);
                    say_where_we_are(&screen);
                }
                Event::KeyDown { keycode: Some(key), repeat: false, .. } => {
                    if key == Keycode::Escape {
                        break 'running;
                    }
                    if let Some(action) = input::action_for_key(&bindings, key) {
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
                act(&mut lib, action);
            }
        }
        repeat.release(&pressed);
        held.clone_from(&pressed);

        draw(&mut canvas, &mut painter, &mut lib, &screen);
        canvas.present();
    }
    Ok(())
}

/// Milliseconds since SDL started, which is what `padpoll` counts in.
fn ticks(sdl: &sdl2::Sdl) -> f64 {
    sdl.timer().map(|t| t.ticks64() as f64).unwrap_or(0.0)
}

/// What the window is, in points.
///
/// The *drawable* size, not the window size: on a retina display those differ
/// by the backing scale, and the drawable is the one with pixels in it.
fn viewport(canvas: &sdl2::render::WindowCanvas, scale: Scale) -> Viewport {
    let (w, h) = canvas.output_size().unwrap_or((POCKET.0, POCKET.1));
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
    use sdl2::pixels::Color;

    pub const BACKGROUND: Color = Color::RGB(18, 18, 22);
    pub const COLUMN: Color = Color::RGB(26, 26, 32);
    pub const CARD: Color = Color::RGB(44, 44, 54);
    pub const CURSOR: Color = Color::RGB(90, 130, 190);
    pub const TEXT: Color = Color::RGB(228, 230, 238);
    pub const DIM: Color = Color::RGB(140, 142, 152);
    pub const FAINT: Color = Color::RGB(96, 98, 108);
}

/// The sizes, in points. Nothing here is a pixel.
mod size {
    pub const CARD: f32 = 150.0;
    pub const GAP: f32 = 14.0;
    pub const PICKER: f32 = 260.0;
    pub const ASIDE: f32 = 320.0;
    /// Cover art is 3:4, so a card is taller than it is wide.
    pub const ART: f32 = CARD / 0.75;
    pub const LABEL: f32 = 13.0;
    /// Two lines of label, and the gap above them.
    pub const CAPTION: f32 = LABEL * 1.3 * 2.0 + 4.0;
    pub const ROW: f32 = 30.0;
    pub const TITLE: f32 = 15.0;
}

fn draw(
    canvas: &mut sdl2::render::WindowCanvas,
    painter: &mut text::Painter,
    lib: &mut library::Library,
    screen: &Viewport,
) {
    canvas.set_draw_color(paint::BACKGROUND);
    canvas.clear();

    let px = |points: f32| screen.scale.px(points);
    let panes = screen.panes();
    let showing_games = lib.view == library::View::Roms;

    // The picker column, where there is room for one. Where there is not, it
    // is the whole pane until a console is opened — which is the one-pane
    // arrangement, and the handheld's.
    let picker_column = panes >= Panes::Two;
    let mut left = 0.0;
    if picker_column {
        canvas.set_draw_color(paint::COLUMN);
        fill(canvas, 0.0, 0.0, px(size::PICKER), screen.height_px);
        left = size::PICKER + size::GAP;
    }
    let mut right = screen.width();
    if panes == Panes::Three {
        canvas.set_draw_color(paint::COLUMN);
        fill(canvas, px(screen.width() - size::ASIDE), 0.0, px(size::ASIDE), screen.height_px);
        right = screen.width() - size::ASIDE - size::GAP;
    }

    if picker_column || !showing_games {
        let width = if picker_column { size::PICKER } else { screen.width() };
        draw_consoles(canvas, painter, lib, screen, width, !showing_games || !picker_column);
    }

    if showing_games {
        let middle = (right - left - size::GAP).max(0.0);
        let columns = (((middle + size::GAP) / (size::CARD + size::GAP)).floor() as usize).max(1);
        lib.relayout(columns);
        draw_games(canvas, painter, lib, screen, left, columns);
    }

    // What the window thinks it is. On screen rather than in the terminal
    // because what is worth watching is what happens while an edge is dragged.
    let filters = lib.filters();
    let readout = format!(
        "{:.0}x{:.0}pt · {:.2}x · {:?} · {} · {}{}",
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
    );
    if let Some(row) = lib.selected() {
        let name = text::Spec::new(&row.name, 12.0, screen.scale.factor());
        painter.draw(canvas, &name, px(size::GAP), screen.height_px - px(size::GAP + 16.0), paint::DIM);
    }
    let spec = text::Spec::new(readout, 11.0, screen.scale.factor());
    let (w, h) = painter.measure(&spec);
    painter.draw(
        canvas,
        &spec,
        screen.width_px - w as f32 - px(size::GAP),
        screen.height_px - h as f32 - px(size::GAP),
        paint::FAINT,
    );
}

/// The consoles, as a column of names.
fn draw_consoles(
    canvas: &mut sdl2::render::WindowCanvas,
    painter: &mut text::Painter,
    lib: &mut library::Library,
    screen: &Viewport,
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
        let on = offset == lib.console_at;
        if on {
            canvas.set_draw_color(if focused { paint::CURSOR } else { paint::CARD });
            fill(canvas, px(size::GAP / 2.0), px(y - 3.0), px(width - size::GAP), px(size::ROW - 2.0));
        }
        let spec = text::Spec::new(&console.name, size::TITLE, screen.scale.factor())
            .wrapped(width - size::GAP * 3.0 - 40.0, 1);
        painter.draw(canvas, &spec, px(size::GAP), px(y), if on { paint::TEXT } else { paint::DIM });

        let count = text::Spec::new(console.games.to_string(), 11.0, screen.scale.factor());
        let (cw, _) = painter.measure(&count);
        painter.draw(
            canvas,
            &count,
            px(width - size::GAP) - cw as f32,
            px(y + 3.0),
            if on { paint::TEXT } else { paint::FAINT },
        );
    }
}

/// One console's games, as covers with their names under them.
fn draw_games(
    canvas: &mut sdl2::render::WindowCanvas,
    painter: &mut text::Painter,
    lib: &mut library::Library,
    screen: &Viewport,
    left: f32,
    columns: usize,
) {
    let px = |points: f32| screen.scale.px(points);
    let step = size::ART + size::CAPTION + size::GAP;
    let visible_rows = ((screen.height() - size::GAP) / step).floor().max(1.0) as usize;
    // The band around the cursor. `visible.js` does this properly for the
    // webview and `slice()` is the thing to port here; this is the two lines
    // that keep the cursor on screen until then.
    let cursor_row = lib.at / columns;
    let first_row = cursor_row.saturating_sub(visible_rows.saturating_sub(1));

    let rows: Vec<_> = lib.showing().map(|r| (r.name.clone(), r.favourite)).collect();
    for (i, (name, favourite)) in rows.iter().enumerate() {
        let (row, col) = (i / columns, i % columns);
        if row < first_row || row >= first_row + visible_rows {
            continue;
        }
        let x = left + size::GAP + col as f32 * (size::CARD + size::GAP);
        let y = size::GAP + (row - first_row) as f32 * step;

        let on = i == lib.at;
        canvas.set_draw_color(if on { paint::CURSOR } else { paint::CARD });
        fill(canvas, px(x), px(y), px(size::CARD), px(size::ART));

        let spec = text::Spec::new(name, size::LABEL, screen.scale.factor())
            .wrapped(size::CARD, 2);
        painter.draw(
            canvas,
            &spec,
            px(x),
            px(y + size::ART + 4.0),
            if on { paint::TEXT } else { paint::DIM },
        );
        if *favourite {
            let star = text::Spec::new("★", 11.0, screen.scale.factor());
            painter.draw(canvas, &star, px(x + 4.0), px(y + 4.0), paint::TEXT);
        }
    }
}

fn fill(canvas: &mut sdl2::render::WindowCanvas, x: f32, y: f32, w: f32, h: f32) {
    let _ = canvas.fill_rect(Rect::new(x as i32, y as i32, w.max(0.0) as u32, h.max(0.0) as u32));
}

/// Unused for now, and kept honest: the tables this front end resolves through
/// are the same ones the desktop app writes.
#[allow(dead_code)]
fn actions() -> &'static [binds::Action] {
    binds::ACTIONS
}
