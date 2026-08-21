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
use sdl2::pixels::Color;
use sdl2::rect::Rect;
use std::collections::BTreeSet;

mod input;
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
                        act(action, now);
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
                act(action, now);
            }
        }
        repeat.release(&pressed);
        held.clone_from(&pressed);

        draw(&mut canvas, &mut painter, &screen, &held);
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
    let cut = SAMPLE
        .iter()
        .filter(|name| {
            painter.is_clipped(&text::Spec::new(**name, 13.0, 1.0).wrapped(150.0, 2))
        })
        .count();
    println!("{cut} of {} sample titles are cut short at a 150pt card", SAMPLE.len());
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

/// Nothing acts yet. Printed rather than swallowed, so a binding that resolves
/// to nothing is visible now rather than at the end of phase four.
fn act(action: &str, now: f64) {
    println!("[{now:>8.0}ms] {action}");
}

/// A grid of cards, in points, and the columns the window has room for.
///
/// Not a design. It is the smallest thing that is wrong on screen if the units
/// are wrong: cards that change physical size when the window is dragged
/// between displays, or a column that appears at the wrong width.
/// Names that between them break every naive way of drawing text.
///
/// Not a placeholder: a Latin title far too long for its card, a Japanese one
/// with no spaces to break at, an accented one, and a short one that must be
/// left alone. If all four look right at every window size, the hard part of
/// phase two is done.
const SAMPLE: &[&str] = &[
    "Metroid",
    "ゼルダの伝説 神々のトライフォース",
    "Castlevania: Symphony of the Night",
    "Pokémon Crystal",
    "ドラゴンクエストIII そして伝説へ",
    "Mortal Kombat II: The Very Long Subtitle Nobody Asked For",
    "Sonic the Hedgehog 2",
    "スーパーマリオブラザーズ3",
];

fn draw(
    canvas: &mut sdl2::render::WindowCanvas,
    painter: &mut text::Painter,
    screen: &Viewport,
    held: &BTreeSet<String>,
) {
    const CARD: f32 = 150.0;
    const GAP: f32 = 14.0;
    const PICKER: f32 = 260.0;
    const ASIDE: f32 = 320.0;
    /// Cover art is 3:4, so a card is taller than it is wide, and the name
    /// sits under it.
    const ART: f32 = CARD / 0.75;
    const LABEL: f32 = 13.0;
    /// Two lines of label, and the gap above them.
    const CAPTION: f32 = LABEL * 1.3 * 2.0 + 4.0;

    canvas.set_draw_color(Color::RGB(18, 18, 22));
    canvas.clear();

    let px = |points: f32| screen.scale.px(points);
    let panes = screen.panes();

    // The picker column, where there is room for one.
    let mut left = 0.0;
    if panes >= Panes::Two {
        canvas.set_draw_color(Color::RGB(30, 30, 38));
        fill(canvas, 0.0, 0.0, px(PICKER), screen.height_px);
        left = PICKER + GAP;
    }
    // The preview, where there is room for that too.
    let mut right = screen.width();
    if panes == Panes::Three {
        canvas.set_draw_color(Color::RGB(30, 30, 38));
        fill(canvas, px(screen.width() - ASIDE), 0.0, px(ASIDE), screen.height_px);
        right = screen.width() - ASIDE - GAP;
    }

    // Covers, at whatever the middle can hold. The same arithmetic
    // `gridnav::uniform` navigates by, which is why it needs no geometry.
    let middle = (right - left - GAP).max(0.0);
    let columns = (((middle + GAP) / (CARD + GAP)).floor() as usize).max(1);
    let lit = if held.is_empty() { 0 } else { 1 };
    for i in 0..(columns * 4) {
        let (row, col) = (i / columns, i % columns);
        let x = left + GAP + col as f32 * (CARD + GAP);
        let y = GAP + row as f32 * (ART + CAPTION + GAP);
        if y + ART + CAPTION > screen.height() {
            break;
        }
        canvas.set_draw_color(if i == lit {
            Color::RGB(90, 130, 190)
        } else {
            Color::RGB(44, 44, 54)
        });
        fill(canvas, px(x), px(y), px(CARD), px(ART));


        // The name, in the width the card actually has, over two lines, cut
        // short with an ellipsis if it does not fit. This is the whole of
        // phase two on screen.
        let name = SAMPLE[i % SAMPLE.len()];
        let spec = text::Spec::new(name, LABEL, screen.scale.factor()).wrapped(CARD, 2);
        painter.draw(
            canvas,
            &spec,
            px(x),
            px(y + ART + 4.0),
            if i == lit { Color::RGB(235, 240, 250) } else { Color::RGB(190, 190, 200) },
        );
    }

    // What the window currently thinks it is, in the corner. On screen rather
    // than in the terminal because the thing worth watching is what happens
    // *while* an edge is being dragged — the moment a column appears, and
    // whether the cards stay the same physical size across displays.
    let readout = format!(
        "{:.0}x{:.0}pt · {:.2}x · {:?} · {} columns",
        screen.width(),
        screen.height(),
        screen.scale.factor(),
        panes,
        columns,
    );
    let spec = text::Spec::new(readout, 11.0, screen.scale.factor());
    let (w, h) = painter.measure(&spec);
    painter.draw(
        canvas,
        &spec,
        screen.width_px - w as f32 - px(GAP),
        screen.height_px - h as f32 - px(GAP),
        Color::RGB(120, 120, 132),
    );
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
