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

use crate::gfx::{Gfx, Rgba};
use anyhow::{Context, Result};
use romm_desktop::layout::{Edges, Rect, Scale, Size, Viewport};
use romm_desktop::{padpoll, rowwindow};
use sdl2::event::{Event, WindowEvent};
use std::collections::BTreeSet;

mod backdrop;
mod covers;
mod gfx;
mod glass;
mod input;
mod keyboard;
mod library;
mod ports;
mod settings;
mod status;
mod text;
mod wifi;

/// The handheld's panel, in points — and the size every layout below is
/// written against.
///
/// The Miyoo Flip is 640x480. That is not a window size, it is the *unit*: on
/// the device one point is one pixel, and on this Mac the window is opened four
/// times larger and `scale_for` divides back down, so the layout is 640x480
/// points either way and what is on screen here is what will be on screen
/// there. Getting that wrong is how a design that reads fine on a desk turns
/// out to be four words per line on the device.
const PANEL: (u32, u32) = (640, 480);

/// How much bigger than the panel the preview window is, in each direction.
///
/// Four, because at 1:1 a 640x480 window on a retina Mac is a postage stamp
/// nobody can judge a layout in. Asked for as *points*, so the window is
/// requested at half this and the display's own doubling supplies the rest —
/// on a retina Mac that lands on 2560x1920 pixels, four device pixels per panel
/// pixel. It does not have to land exactly: `viewport` divides whatever
/// drawable it gets by 640, so the layout is a 640-point panel either way and
/// only the physical size of the preview changes.
const PREVIEW: u32 = 4;

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
    let sdl = sdl2::init()
        .map_err(anyhow::Error::msg)
        .context("starting SDL")?;
    let video = sdl
        .video()
        .map_err(anyhow::Error::msg)
        .context("opening the display")?;

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

    let window = open_window(&video)
        .map_err(anyhow::Error::msg)
        .context("opening a window")?;

    // The config, read once. Everything the front end needs from it is settled
    // before the first frame; nothing below re-reads a file.
    // This front end's own file — see `settings::FILE` for why it is not
    // `config.toml`.
    let (config, config_from) = settings::load();
    println!("config: {config_from}");

    // The context, ours. Everything below draws through it.
    let _context = window
        .gl_create_context()
        .map_err(anyhow::Error::msg)
        .context("creating an OpenGL context")?;
    window
        .gl_set_context_to_current()
        .map_err(anyhow::Error::msg)?;
    let mut gfx = unsafe { Gfx::new(&video) }.context("setting up the renderer")?;
    println!(
        "gl: {} · {}",
        unsafe { gfx::reported(gl::VERSION) },
        unsafe { gfx::reported(gl::SHADING_LANGUAGE_VERSION) }
    );
    // Vertical sync, so a menu does not run the fan up.
    let _ = video.gl_set_swap_interval(sdl2::video::SwapInterval::VSync);

    let mut painter = text::Painter::new().context("finding fonts")?;
    check_fonts(&mut painter);

    // The shader backdrop, underneath everything. Built after the renderer, so
    // the context it compiles against is the one the renderer made current.
    //
    // Not fatal: a machine whose driver will not compile it still gets a
    // library, and the message says why rather than the window being black.
    // The frosted panels. Not fatal either: a driver that will not draw into a
    // texture still gets a library, with flat panels instead of glass.
    let mut frosted = match unsafe { glass::Glass::new(PANEL.0 * PREVIEW, PANEL.1 * PREVIEW) } {
        Ok(mut g) => {
            g.strength = config.appearance.glass as f32 / 20.0;
            Some(g)
        }
        Err(e) => {
            eprintln!("no glass: {e:#}");
            None
        }
    };

    // The style and its tuning come from the config, so the Appearance settings
    // are the ones that draw this rather than a second set that agrees by
    // coincidence.
    let mut backdrop =
        match unsafe { backdrop::Backdrop::build(&video, &config.appearance.backdrop) } {
            Ok(mut b) => {
                b.scheme = *backdrop::scheme(&config.appearance.scheme);
                b.speed = config.appearance.backdrop_speed as f32 / 100.0;
                b.strength = config.appearance.backdrop_strength as f32 / 100.0;
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
        config.library.romm_collections,
    )
    .context("opening the library")?;
    if lib.consoles.is_empty() {
        eprintln!(
            "warning: no consoles in {}/cache.sqlite3 — run `romm-desktop sync` to fill it",
            std::env::current_dir().unwrap_or_default().display()
        );
    }
    println!("{} consoles", lib.consoles.len());
    // The consoles this library actually holds, so the Emulators pane offers
    // rows for those and not for the whole core map.
    let consoles: Vec<(String, String)> = lib
        .consoles
        .iter()
        .map(|c| (c.slug.clone(), c.name.clone()))
        .collect();
    let core_map = romm_desktop::coremap::CoreMap::load_or_embedded(std::path::Path::new(
        "data/esde-core-map.json",
    ));
    lib.panes = settings::panes(&config, &consoles, &core_map);
    start_at(&mut lib);

    let mut screen = viewport(&window);
    say_where_we_are(&screen);

    // The bindings the desktop app writes. Resolved once — the loop below runs
    // at the display's refresh rate, and this is a scan of two tables.
    let bindings = config.bindings.clone();
    // Read through the face-button swap, which is this app's own and goes no
    // further — see .
    let pad_map = bindings.pad_map_swapped(config.controllers.swap_ab, config.controllers.swap_xy);

    let controller = sdl.game_controller().map_err(anyhow::Error::msg)?;
    let mut pads = input::Pads::open_first(&controller);
    let mut repeat = padpoll::Repeat::default();

    let timer = sdl
        .timer()
        .map_err(anyhow::Error::msg)
        .context("starting the clock")?;
    let mut events = sdl.event_pump().map_err(anyhow::Error::msg)?;
    // What the pad is holding, so the drawing can show that input arrived.
    let mut held: BTreeSet<String> = BTreeSet::new();

    // Where each thing was drawn last frame, so the mouse knows what it is
    // over. Filled by the drawing and read by the next event — which is the
    // only order that works, since what is on screen is decided while drawing
    // it.
    let mut hits: Hits = Hits::default();
    // What the pointer is over, which is not what is chosen.
    let mut hover: Option<usize> = None;

    // The open text field, when something asked for one. While it is here the
    // pad drives the keyboard and nothing else — a field you are typing into
    // owns the input until it is finished with.
    let mut typing: Option<keyboard::Keyboard> = demo_keyboard();
    // The corner: clock, signal, charge. Read on a timer, not per frame.
    let mut status = status::Status::default();
    // What the renderer is currently set to, so a change can be spotted without
    // rebuilding a shader every frame.
    // Seeded from what the renderer was actually built with, not from the
    // settings entries. Reading the entries here makes the two agree by
    // definition — including when they do not, which is how a change made
    // before the loop starts was written to the file and never drawn.
    let mut showing = settings::Look {
        backdrop: config.appearance.backdrop.clone(),
        scheme: config.appearance.scheme.clone(),
        speed: config.appearance.backdrop_speed as f32 / 100.0,
        strength: config.appearance.backdrop_strength as f32 / 100.0,
        glass: config.appearance.glass,
    };

    let mut frames = 0u32;

    'running: loop {
        let now = ticks(&timer);

        for event in events.poll_iter() {
            match event {
                Event::Quit { .. } => break 'running,
                Event::Window {
                    win_event: WindowEvent::SizeChanged(..),
                    ..
                }
                | Event::Window {
                    win_event: WindowEvent::Moved(..),
                    ..
                } => {
                    // Moved as well as resized: dragging a window between a
                    // retina display and a plain one changes the scale
                    // without changing the size in points.
                    screen = viewport(&window);
                    say_where_we_are(&screen);
                }
                // The mouse. ES-DE makes this hard and it is the complaint
                // Frank had about it; here it is the same cursor the pad
                // moves, pointed at instead of stepped to.
                // Moving the pointer lights a row up. It does not *choose*
                // it: choosing scrolls the list to keep the choice on screen,
                // the scroll moves a different row under the pointer, and the
                // next motion event chooses that one — so the grid ran away
                // from the mouse. `.row:hover` in the stylesheet is a
                // background color and nothing else, for the same reason.
                Event::MouseMotion { x, y, .. } => {
                    let (x, y) = pointer(&window, x, y);
                    hover = hits.at(x, y);
                }
                Event::MouseButtonDown {
                    mouse_btn, x, y, ..
                } => match mouse_btn {
                    sdl2::mouse::MouseButton::Left => {
                        let (x, y) = pointer(&window, x, y);
                        if let Some(action) = hits.button_at(x, y) {
                            act(&mut lib, action);
                        } else if let Some(tab) = hits.tab_at(x, y) {
                            lib.go_to_section(tab);
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
                Event::KeyDown {
                    keycode: Some(key),
                    repeat: false,
                    ..
                } => {
                    // Escape is bound to Back, not to quit — it was hardcoded
                    // here in the first commit and never taken out, so going
                    // back from a console closed the app instead. The window
                    // closes the window; Cmd-Q and Alt-F4 do what they always
                    // do.
                    if let Some(action) = input::action_for_key(&bindings, key) {
                        if let Some(kb) = typing.as_mut() {
                            match kb.act(action) {
                                keyboard::Outcome::Done => {
                                    let text = kb.text.clone();
                                    if let Some(target) = kb.target.clone() {
                                        lib.take_typed(&target, &text);
                                    }
                                    typing = None;
                                }
                                keyboard::Outcome::Cancelled => typing = None,
                                keyboard::Outcome::Typing => {}
                            }
                            continue;
                        }
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
        // While a control is being captured, a button is a button and not
        // whatever it is bound to — that is the whole point of capturing.
        if lib.capturing.is_some()
            && let Some(button) = pads.any_button()
        {
            match button {
                // B clears, Start leaves it alone. Everything else binds.
                1 => lib.capture_button(None),
                9 => lib.cancel_capture(),
                other => lib.capture_button(Some(other)),
            }
            repeat.release(&BTreeSet::new());
            continue;
        }

        let pressed = pads.pressed(&pad_map);
        for action in &pressed {
            if repeat.fire(action, now) {
                if let Some(kb) = typing.as_mut() {
                    match kb.act(action) {
                        keyboard::Outcome::Done => {
                            let text = kb.text.clone();
                            if let Some(target) = kb.target.clone() {
                                lib.take_typed(&target, &text);
                            }
                            typing = None;
                        }
                        keyboard::Outcome::Cancelled => typing = None,
                        keyboard::Outcome::Typing => {}
                    }
                    continue;
                }
                if matches!(action.as_str(), "back" | "back2") && lib.at_top() {
                    break 'running;
                }
                act(&mut lib, action);
            }
        }
        repeat.release(&pressed);
        held.clone_from(&pressed);

        if typing.is_none()
            && let Some(kb) = keyboard_wanted(&mut lib)
        {
            typing = Some(kb);
        }
        // A scan or a join, finishing on its thread. Polled here rather than
        // in the drawing so a slow helper never holds up a frame.
        if let Some(job) = lib.wifi_job.as_ref()
            && let Some(state) = job.poll()
        {
            lib.wifi = state;
            lib.wifi_job = None;
            lib.wifi_at = 0;
            lib.refresh_rows();
        }
        // A setting is not a setting until it does something. The Appearance
        // pane writes the file; this is what makes the screen change while you
        // are looking at it.
        let want = settings::look(&lib.panes);
        if want != showing {
            if want.backdrop != showing.backdrop {
                match unsafe { backdrop::Backdrop::build(&video, &want.backdrop) } {
                    Ok(b) => backdrop = Some(b),
                    Err(e) => eprintln!("no backdrop {}: {e:#}", want.backdrop),
                }
            }
            if let Some(g) = frosted.as_mut() {
                // 0 to 60 on the slider, the same range the webview uses, onto
                // how far each tap reaches.
                g.strength = want.glass as f32 / 20.0;
            }
            if let Some(b) = backdrop.as_mut() {
                b.scheme = *backdrop::scheme(&want.scheme);
                b.speed = want.speed;
                b.strength = want.strength;
            }
            showing = want;
        }

        status.poll(now as u64);
        // Only the panel, centered. The rest of the window stays as it was
        // cleared, which is what letterboxing looks like.
        let (dw, dh) = window.drawable_size();
        let (ox, oy, _) = panel_box(dw as f32, dh as f32);
        gfx.resize_at(ox, oy, screen.width_px, screen.height_px);

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
        hits = draw(
            &gfx,
            frosted.as_ref(),
            &mut painter,
            &mut art,
            &mut lib,
            &screen,
            hover,
            typing.as_ref(),
            &status,
        );
        window.gl_swap_window();

        // A portrait, and then out. Taken after a handful of frames rather
        // than the first: covers are decoded as they are asked for, so frame
        // one is a picture of an empty cache.
        if let Some((path, _)) = shot_wanted() {
            frames += 1;
            // Longer than eight when asked: taking a portrait is one job, and
            // watching what the process weighs while it draws is another.
            let want: u32 = std::env::var("ROMM_SDL_FRAMES")
                .ok()
                .and_then(|n| n.parse().ok())
                .unwrap_or(8);
            if frames >= want {
                unsafe { gl::Finish() };
                save_shot(&window, &path).with_context(|| format!("saving {path}"))?;
                println!("shot: {path}");
                break 'running;
            }
        }
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

/// A portrait of one frame, written to a file, and then nothing.
///
/// `ROMM_SDL_SHOT=/tmp/a.png romm-sdl` draws the interface once and saves it.
/// It exists because the alternative was Frank opening the app and describing
/// what looked wrong, and a description is not a pixel — the console grid
/// spilling out of its panel took four rounds of that, and one look at a file
/// to find.
///
/// `ROMM_SDL_SIZE=1460x1046` sets the window; the default is the handheld's.
fn shot_wanted() -> Option<(String, (u32, u32))> {
    let path = std::env::var("ROMM_SDL_SHOT").ok()?;
    let size = std::env::var("ROMM_SDL_SIZE")
        .ok()
        .and_then(|s| {
            let (w, h) = s.split_once(['x', 'X'])?;
            Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
        })
        .unwrap_or((PANEL.0 * PREVIEW / 2, PANEL.1 * PREVIEW / 2));
    Some((path, size))
}

/// Read the finished frame back and write it out.
///
/// GL hands rows back bottom-up, so they are flipped — the sort of thing that
/// makes a picture look fine until you read a word in it.
fn save_shot(window: &sdl2::video::Window, path: &str) -> Result<()> {
    use sdl2::image::SaveSurface;
    let (w, h) = window.drawable_size();
    let mut flipped = vec![0u8; (w * h * 4) as usize];
    unsafe {
        gl::PixelStorei(gl::PACK_ALIGNMENT, 1);
        gl::ReadPixels(
            0,
            0,
            w as i32,
            h as i32,
            gl::RGBA,
            gl::UNSIGNED_BYTE,
            flipped.as_mut_ptr() as *mut _,
        );
    }
    let stride = (w * 4) as usize;
    let mut rows = Vec::with_capacity(flipped.len());
    for row in (0..h as usize).rev() {
        rows.extend_from_slice(&flipped[row * stride..row * stride + stride]);
    }
    let surface = sdl2::surface::Surface::from_data(
        &mut rows,
        w,
        h,
        stride as u32,
        sdl2::pixels::PixelFormatEnum::ABGR8888,
    )
    .map_err(anyhow::Error::msg)?;
    surface.save(path).map_err(anyhow::Error::msg)?;
    Ok(())
}

fn open_window(video: &sdl2::VideoSubsystem) -> Result<sdl2::video::Window, String> {
    let shot = shot_wanted();
    let (w, h) = shot
        .as_ref()
        .map(|(_, size)| *size)
        .unwrap_or((PANEL.0 * PREVIEW / 2, PANEL.1 * PREVIEW / 2));
    let mut builder = video.window("RomM", w, h);
    // Taking a portrait should not throw a window at whoever is using the
    // machine, so it happens off screen.
    if shot.is_some() {
        builder.hidden();
    }
    builder
        .position_centered()
        .resizable()
        // Retina and the like. Without this the window is described in points
        // by the platform and drawn at half the resolution it could be.
        .allow_highdpi()
        .opengl()
        .build()
        .map_err(|e| e.to_string())
}

/// Which tab and console to open on, for previews.
///
/// `ROMM_SDL_TAB=library ROMM_SDL_OPEN=megadrive` opens that console's games;
/// `ROMM_SDL_TAB=mine ROMM_SDL_OPEN=user` opens that kind of collection. Only
/// useful for judging a screen without pressing anything to get to it — the app
/// opens on the first tab otherwise.
fn start_at(lib: &mut library::Library) {
    if let Ok(want) = std::env::var("ROMM_SDL_TAB")
        && let Some(i) = library::SECTIONS.iter().position(|s| s.id == want)
    {
        lib.go_to_section(i);
    }
    let Ok(open) = std::env::var("ROMM_SDL_OPEN") else {
        return;
    };
    // What "open" names depends on the tab: a console in the Library, a kind
    // of collection in Collections.
    let err = if lib.section().id == "settings" {
        lib.pane_at = lib.panes.iter().position(|p| p.id == open).unwrap_or(0);
        lib.act("activate").map(|_| ())
    } else if lib.section().id == "mine" {
        lib.shelf_at = lib
            .shelves
            .iter()
            .position(|sh| sh.name().eq_ignore_ascii_case(&open))
            .unwrap_or(0);
        lib.open_shelf()
    } else {
        let at = open
            .parse::<usize>()
            .ok()
            .or_else(|| lib.consoles.iter().position(|c| c.slug == open))
            .unwrap_or(0);
        lib.console_at = at.min(lib.consoles.len().saturating_sub(1));
        // Through `activate`, not `open_console`: Ports and Tools are consoles
        // that open a list of scripts instead, and a preview that takes a
        // different path from the app is a preview of something else.
        lib.act("activate").map(|_| ())
    };
    if let Err(e) = err {
        eprintln!("ROMM_SDL_OPEN={open}: {e:#}");
    }
    // Replay a few presses, for judging a screen that takes some getting to —
    // a list of options is three presses in and cannot be screenshotted
    // otherwise. `ROMM_SDL_ACT=down,down,activate`, in the app's own action
    // names, through the app's own dispatcher.
    if let Ok(script) = std::env::var("ROMM_SDL_ACT") {
        for action in script.split(',').map(str::trim).filter(|a| !a.is_empty()) {
            if let Err(e) = lib.act(action) {
                eprintln!("ROMM_SDL_ACT {action}: {e:#}");
            }
        }
    }
}

/// Milliseconds since SDL started, which is what `padpoll` counts in.
///
/// The subsystem is held rather than asked for each frame: `Sdl::timer`
/// initializes it, and doing that sixty times a second for a number is work
/// for nothing — and if it ever failed, time would silently stop and the
/// backdrop would freeze with nothing to say why.
fn ticks(timer: &sdl2::TimerSubsystem) -> f64 {
    timer.ticks64() as f64
}

/// What the window is, in points.
///
/// The *drawable* size, not the window size: on a retina display those differ
/// by the backing scale, and the drawable is the one with pixels in it.
fn viewport(window: &sdl2::video::Window) -> Viewport {
    let (w, h) = window.drawable_size();
    let (_, _, scale) = panel_box(w as f32, h as f32);
    Viewport::new(
        PANEL.0 as f32 * scale.factor(),
        PANEL.1 as f32 * scale.factor(),
        scale,
    )
}

/// Where the panel sits in the window, and how big a pixel is.
///
/// A whole-number scale, and letterboxed. `Scale::new` snaps to quarter steps
/// and clamps at four, so dividing the drawable by 640 gave 696 points on a
/// 2784-pixel window — a preview of a screen the device does not have, with a
/// different number of panes. Fitting the panel and leaving the rest of the
/// window blank is the only arrangement where what is on screen here is what
/// will be on screen there.
fn panel_box(w: f32, h: f32) -> (f32, f32, Scale) {
    let fit = (w / PANEL.0 as f32)
        .min(h / PANEL.1 as f32)
        .floor()
        .max(1.0);
    let (pw, ph) = (PANEL.0 as f32 * fit, PANEL.1 as f32 * fit);
    (
        ((w - pw) / 2.0).max(0.0),
        ((h - ph) / 2.0).max(0.0),
        Scale::new(fit),
    )
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
            let note = if face == asked || asked == "(any)" {
                ""
            } else {
                "  <- not what was asked for"
            };
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

/// A keyboard for whichever setting asked for one, if any.
///
/// Taken rather than read: one activation opens one keyboard, and leaving the
/// request in place would reopen it the moment the last one closed.
fn keyboard_wanted(lib: &mut library::Library) -> Option<keyboard::Keyboard> {
    let want = lib.wants_keyboard.take()?;
    Some(keyboard::Keyboard::new(want.prompt, &want.value, want.secret).filling(want.target))
}

/// A grid of cards, in points, and the columns the window has room for.
///
/// Not a design. It is the smallest thing that is wrong on screen if the units
/// are wrong: cards that change physical size when the window is dragged
/// between displays, or a column that appears at the wrong width.
/// The colors, in one place.
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
    /// The plate a mark sits on, so it reads against artwork of any color.
    /// `.row:hover` — the pointer saying where it is, which is not the same
    /// as the cursor saying what is chosen.
    pub const HOVER: Rgba = Rgba(1.0, 1.0, 1.0, 0.07);
    pub const STAR: Rgba = Rgba::rgb(240, 200, 90);
    /// A hairline between two halves of one list. Faint on purpose: it is
    /// separating things, not labelling them.
    /// Behind a keyboard: dark enough that the page under it stops competing.
    pub const SCRIM: Rgba = Rgba(0.02, 0.02, 0.04, 0.95);
    /// The keyboard's own panel. Opaque, not the translucent BAR — a game list
    /// showing through the keys is exactly what makes them hard to read.
    pub const SHEET: Rgba = Rgba::rgb(18, 19, 26);
    pub const RULE: Rgba = Rgba(1.0, 1.0, 1.0, 0.13);
    /// On this machine, and on the server.
    pub const HERE: Rgba = Rgba::rgb(120, 200, 140);
    pub const AWAY: Rgba = Rgba::rgb(150, 160, 180);
}

/// The sizes, in points. Nothing here is a pixel.
mod size {
    use romm_desktop::layout::Edges;

    // Everything here is in panel points, and the panel is 640x480. That is
    // the whole budget: a 42-point tab row is a *tenth of the screen height*,
    // which is what the desk-sized numbers these replaced were spending.
    pub const GAP: f32 = 6.0;
    /// The tab row. Six tabs across 640 points, so ~104 each.
    pub const TABS: f32 = 26.0;
    /// The line under the tabs saying where you are.
    pub const HEADER: f32 = 20.0;
    /// The strip along the bottom saying what the buttons do.
    pub const HELP: f32 = 18.0;
    /// The info pane beside the games. Wide enough for a wrapped title and a
    /// column of facts, and no wider — what is left is the list, and the list
    /// is the thing being read.
    pub const ASIDE: f32 = 224.0;
    pub const CARD: f32 = 96.0;
    pub const LABEL: f32 = 12.0;
    pub const TITLE: f32 = 13.0;
    /// A list row. Tall enough to touch, short enough that a 434-point body
    /// shows sixteen of them.
    pub const ROW: f32 = 26.0;
    /// A console tile: the machine's picture above, name and count below.
    pub const TILE: f32 = 148.0;
    pub const TILE_ART: f32 = 66.0;
    /// Two lines of console name — "Nintendo Entertainment System" needs
    /// both — then the game count under it, then the tile's own padding.
    /// Reserving less is what made the count bleed out of the tile and sit
    /// under the row below.
    pub const TILE_CAPTION: f32 = LABEL * 1.3 * 2.0 + 4.0 + 14.0 + 10.0;
    pub const ROUND: f32 = 6.0;
    pub const ROUND_SMALL: f32 = 4.0;
    pub const PAD: Edges = Edges::all(GAP);
}

/// Where each thing was drawn, so the mouse knows what it is over.
///
/// Rebuilt every frame, because that is when the answer is known, and in
/// pixels because that is what the pointer arrives in.
#[derive(Default)]
pub struct Hits {
    rows: Vec<(Rect, usize)>,
    tabs: Vec<(Rect, usize)>,
    /// Header controls, each remembered as the action it fires.
    ///
    /// One list rather than one field per button: a control in the header is
    /// a name and a rectangle, and everything it does is already an `act`
    /// case because the pad has to be able to do it too. Adding a button is
    /// then a call, not a field, a variant and a branch.
    buttons: Vec<(Rect, &'static str)>,
}

impl Hits {
    fn row(&mut self, at: Rect, index: usize) {
        self.rows.push((at, index));
    }

    fn tab(&mut self, at: Rect, index: usize) {
        self.tabs.push((at, index));
    }

    fn button_at(&self, x: f32, y: f32) -> Option<&'static str> {
        self.buttons
            .iter()
            .find(|(r, _)| r.contains(x, y))
            .map(|(_, a)| *a)
    }

    fn at(&self, x: f32, y: f32) -> Option<usize> {
        self.rows
            .iter()
            .find(|(r, _)| r.contains(x, y))
            .map(|(_, i)| *i)
    }

    /// Checked before the rows, because the tab row is drawn over them.
    fn tab_at(&self, x: f32, y: f32) -> Option<usize> {
        self.tabs
            .iter()
            .find(|(r, _)| r.contains(x, y))
            .map(|(_, i)| *i)
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
    /// Which row the pointer is over, if any. Separate from the cursor: see
    /// the motion handler.
    hover: Option<usize>,
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

    fn fill(&self, at: Rect, color: Rgba, round: f32) {
        let at = self.px(at);
        let gfx = self.gfx;
        gfx.rounded(round * self.screen.scale.factor(), || gfx.fill(at, color));
    }

    fn outline(&self, at: Rect, thickness: f32, color: Rgba, round: f32) {
        let at = self.px(at);
        let s = self.screen.scale.factor();
        let gfx = self.gfx;
        gfx.rounded(round * s, || gfx.outline(at, thickness * s, color));
    }

    fn spec(&self, text: impl Into<String>, size: f32) -> text::Spec {
        text::Spec::new(text, size, self.screen.scale.factor())
    }

    fn wrapped(&self, text: impl Into<String>, size: f32, width: f32, lines: u16) -> text::Spec {
        self.spec(text, size).wrapped(width, lines)
    }

    /// A label at the top left of a box. Returns its height, in points.
    fn label(&mut self, spec: &text::Spec, at: Rect, color: Rgba) -> f32 {
        let at = self.px(at);
        let h = self.painter.put(self.gfx, spec, at, color);
        self.screen.scale.pt(h)
    }

    fn label_right(&mut self, spec: &text::Spec, at: Rect, color: Rgba) {
        let at = self.px(at);
        self.painter.put_right(self.gfx, spec, at, color);
    }

    fn label_centered(&mut self, spec: &text::Spec, at: Rect, color: Rgba) {
        let at = self.px(at);
        self.painter.put_centered(self.gfx, spec, at, color);
    }

    /// The pointer's mark on a row, drawn under everything else in it.
    fn hovering(&self, index: usize, at: Rect, round: f32) {
        if self.hover == Some(index) {
            self.fill(at, paint::HOVER, round);
        }
    }

    /// A header control: a pill with a word in it, and a rectangle the mouse
    /// can find it by.
    ///
    /// The pad reaches these through their bindings, so the button carries the
    /// action name rather than a closure — one string is the label, the hit
    /// and the behavior.

    /// How wide a pill has to be to hold a word.

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
    /// A picture named by path — a port's artwork, which the gamelist points
    /// straight at rather than filing under a platform and a stem.
    fn picture(&mut self, key: i64, path: &std::path::Path, at: Rect, round: f32) -> bool {
        let at = self.px(at);
        let round = round * self.screen.scale.factor();
        let gfx = self.gfx;
        match self.art.by_path(gfx, key, path) {
            Some(art) => {
                gfx.rounded(round, || gfx.picture(art, at, Rgba::WHITE));
                true
            }
            None => false,
        }
    }

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
    hover: Option<usize>,
    typing: Option<&keyboard::Keyboard>,
    status: &status::Status,
) -> Hits {
    let mut f = Frame {
        gfx,
        glass: frosted,
        painter,
        art,
        screen,
        hover,
        hits: Hits::default(),
    };
    let page = Rect::new(0.0, 0.0, screen.width(), screen.height());

    let boxes = page.column(
        0.0,
        &[
            Size::Fixed(size::TABS),
            Size::Fixed(size::HEADER),
            Size::Grow(1.0),
            Size::Fixed(size::HELP),
        ],
    );
    let [tabs, header, body, help] =
        <[Rect; 4]>::try_from(boxes).unwrap_or([Rect::new(0.0, 0.0, 0.0, 0.0); 4]);

    // One page at a time, chosen by the tab. There is no Sofa-or-Desk here and
    // no column count to work out: the panel is 640 points wide and that is
    // room for one list and one pane beside it, so the arrangement is not a
    // question the window gets to answer differently on different days.
    match library::SECTIONS[lib.section.min(library::SECTIONS.len() - 1)].id {
        "library" => draw_library(&mut f, lib, body.inset(size::PAD)),
        "mine" => draw_collections(&mut f, lib, body.inset(size::PAD)),
        "history" => draw_history(&mut f, lib, body.inset(size::PAD)),
        "syncing" => draw_syncing(&mut f, lib, body.inset(size::PAD)),
        "settings" => draw_settings(&mut f, lib, body.inset(size::PAD)),
        // Until Settings is built, its tab is where the keyboard can be looked
        // at — it is the only screen that will ask for one.
        other => draw_unbuilt(&mut f, other, body),
    }

    // What the buttons do here. Said rather than left to be found by pressing
    // everything, which is what a settings screen with no help bar teaches.
    draw_help(&mut f, hints_for(lib, typing.is_some()), help);

    // The keyboard sits over whatever asked for it.
    if let Some(picker) = lib.picking.as_ref() {
        draw_picker_sheet(&mut f, picker, page);
    } else if let Some(cap) = lib.capturing.as_ref() {
        draw_capture(&mut f, cap, page);
    }
    if let Some(kb) = typing {
        draw_keyboard(&mut f, kb, page);
    }

    draw_chrome(&mut f, lib, tabs, header, status);
    f.hits
}

/// The Library tab: consoles, then one console's games with a pane beside them.
///
/// Two screens rather than two columns. 640 points will not hold a console
/// picker *and* a game list *and* an info pane, so the picker is the whole page
/// until a console is opened and then gets out of the way — which is also how
/// the handheld wants to be read, one thing at a time.
fn draw_library(f: &mut Frame, lib: &mut library::Library, area: Rect) {
    if lib.consoles.is_empty() {
        let spec = f.wrapped(
            "No consoles in this library.\nRun `romm-desktop sync` to fill it.",
            size::TITLE,
            area.w * 0.8,
            2,
        );
        f.label_centered(&spec, area, paint::DIM);
        return;
    }

    if lib.view == library::View::Scripts {
        // A port is a name and, if it was scraped, a picture. No size, no
        // console, no star — none of those mean anything for a shell script.
        let [list, aside] = <[Rect; 2]>::try_from(
            area.row(size::GAP, &[Size::Grow(1.0), Size::Fixed(size::ASIDE)]),
        )
        .unwrap();
        let rows: Vec<_> = lib
            .ports
            .iter()
            .map(|p| (p.name.clone(), String::new()))
            .collect();
        draw_picker(f, list, &rows, lib.port_at, rows.len());

        f.pane(aside, paint::COLUMN, size::ROUND);
        if let Some(port) = lib.ports.get(lib.port_at) {
            let inner = aside.inset(size::PAD);
            let (art, below) = inner.split_top(inner.w * 0.62);
            // Negative keys: the cover cache is keyed by ROM id and a port has
            // none, so these take slots no ROM can reach.
            let key = -(lib.port_at as i64 + 1);
            let drawn = port
                .image
                .as_deref()
                .is_some_and(|p| f.picture(key, p, art, size::ROUND_SMALL));
            if !drawn {
                f.pane(art, paint::CARD, size::ROUND_SMALL);
            }
            let title = f.wrapped(&port.name, size::TITLE, below.w, 3);
            f.label(
                &title,
                below.inset(Edges {
                    top: 8.0,
                    ..Edges::default()
                }),
                paint::TEXT,
            );
        }
        return;
    }
    if lib.view != library::View::Roms {
        draw_consoles(f, lib, area, true);
        return;
    }

    // The games, and the facts about the one under the cursor. The list is
    // always a list here — a wall of covers and a sidebar do not both fit, and
    // between the two the list is what answers "what is in this console".
    let [list, aside] =
        <[Rect; 2]>::try_from(area.row(size::GAP, &[Size::Grow(1.0), Size::Fixed(size::ASIDE)]))
            .unwrap();
    draw_game_list(f, lib, list);
    f.pane(aside, paint::COLUMN, size::ROUND);
    draw_detail(f, lib, aside.inset(size::PAD));
}

/// The Collections tab: kinds, then collections, then games.
///
/// Three levels because the data is three levels and the middle one is not
/// optional — there are 1,024 company collections in this library, which is not
/// a list anybody scrolls to the bottom of.
fn draw_collections(f: &mut Frame, lib: &mut library::Library, area: Rect) {
    match lib.view {
        library::View::Groups => {
            let rows: Vec<_> = lib
                .shelves
                .iter()
                .map(|sh| (sh.name().to_owned(), sh.count().to_string()))
                .collect();
            draw_shelf(f, area, &rows, lib.shelf_at, lib.mine_count);
        }
        library::View::Collections => {
            let rows: Vec<_> = lib
                .cols
                .iter()
                .map(|c| (c.name.clone(), format!("{}", c.rom_count)))
                .collect();
            draw_picker(f, area, &rows, lib.col_at, lib.cols.len());
        }
        // A collection of games is a list of games: same list, same info pane.
        library::View::Roms => {
            let [list, aside] = <[Rect; 2]>::try_from(
                area.row(size::GAP, &[Size::Grow(1.0), Size::Fixed(size::ASIDE)]),
            )
            .unwrap();
            draw_game_list(f, lib, list);
            f.pane(aside, paint::COLUMN, size::ROUND);
            draw_detail(f, lib, aside.inset(size::PAD));
        }
        // Not reachable: `go_to_section` puts this tab at `Groups` and only
        // `back`/`activate` move it. Drawing the root beats drawing nothing.
        _ => draw_picker(f, area, &[], 0, 0),
    }
}

/// The History tab: what was actually played, most time first.
///
/// Not the same list as Continue. Continue answers "what was I doing"; this
/// answers "what have I played", which wants time and counts rather than
/// artwork, and reads as a table.
fn draw_history(f: &mut Frame, lib: &mut library::Library, area: Rect) {
    if lib.history.is_empty() {
        let spec = f.wrapped(
            "Nothing played yet.\nPlay time arrives with a sync.",
            size::TITLE,
            area.w * 0.8,
            2,
        );
        f.label_centered(&spec, area, paint::DIM);
        return;
    }
    let [list, aside] =
        <[Rect; 2]>::try_from(area.row(size::GAP, &[Size::Grow(1.0), Size::Fixed(size::ASIDE)]))
            .unwrap();

    let step = size::ROW;
    lib.relayout(1);
    let fits = (list.h / step).floor().max(1.0) as usize;
    let first = lib.history_at.saturating_sub(fits.saturating_sub(1));

    for (offset, game) in lib.history.iter().enumerate().skip(first).take(fits) {
        let line = Rect::new(
            list.x,
            list.y + (offset - first) as f32 * step,
            list.w,
            step,
        );
        if line.bottom() > list.bottom() {
            break;
        }
        f.hits.row(f.px(line), offset);
        let on = offset == lib.history_at;
        f.hovering(offset, line, size::ROUND_SMALL);
        if on {
            f.pane(line, paint::CURSOR, size::ROUND_SMALL);
        }

        // Name and time played. The date moved to the pane: it is the least
        // useful of the three at a glance and was taking a column from the
        // titles, which are what you are reading down.
        let inside = line.inset(Edges::xy(8.0, 5.0));
        let [title, time] =
            <[Rect; 2]>::try_from(inside.row(8.0, &[Size::Grow(1.0), Size::Fixed(56.0)])).unwrap();
        let spec = f.wrapped(&game.name, size::LABEL, title.w, 1);
        f.label(&spec, title, if on { paint::TEXT } else { paint::DIM });
        let spec = f.spec(played_for(game.seconds), 10.0);
        f.label_right(&spec, time, if on { paint::TEXT } else { paint::DIM });
    }

    // What the highlighted game is, in full.
    f.pane(aside, paint::COLUMN, size::ROUND);
    if let Some(game) = lib.history.get(lib.history_at) {
        let inner = aside.inset(size::PAD);
        let (art, below) = inner.split_top(inner.w * 0.62);
        let key = -(lib.history_at as i64 + 1_000_001);
        if !f.cover(game.id, &game.platform, &game.stem, art, size::ROUND_SMALL) {
            f.pane(art, paint::CARD, size::ROUND_SMALL);
        }
        let _ = key;
        let title = f.wrapped(&game.name, size::TITLE, below.w, 2);
        let used = f.label(
            &title,
            below.inset(Edges {
                top: 8.0,
                ..Edges::default()
            }),
            paint::TEXT,
        );

        let facts = [
            ("Console", game.platform.clone()),
            ("Played", played_for(game.seconds)),
            (
                "Times",
                if game.runs == 1 {
                    "once".to_owned()
                } else {
                    format!("{}", game.runs)
                },
            ),
            ("Last", game.last.chars().take(10).collect::<String>()),
        ];
        for (i, (label, value)) in facts.iter().enumerate() {
            let line = Rect::new(
                below.x,
                below.y + used + 16.0 + i as f32 * 18.0,
                below.w,
                16.0,
            );
            let [name, val] =
                <[Rect; 2]>::try_from(line.row(6.0, &[Size::Fixed(58.0), Size::Grow(1.0)]))
                    .unwrap();
            let spec = f.spec(*label, size::LABEL);
            f.label(&spec, name, paint::FAINT);
            let spec = f.spec(value.as_str(), size::LABEL);
            f.label(&spec, val, paint::DIM);
        }
    }
}

/// The Syncing tab: the state a sync would change.
///
/// Nothing here performs one — that needs the network on a thread the draw loop
/// does not have yet. What it does is stop the tab being a lie: everything shown
/// is read from the same cache the other front ends sync into, so it says
/// truthfully how far behind this device is.
fn draw_syncing(f: &mut Frame, lib: &library::Library, area: Rect) {
    let s = &lib.sync;
    let (games, sessions, seconds) = (s.games_played, s.sessions, s.seconds_played);
    let facts: [(&str, String); 6] = [
        (
            "Server",
            s.server_version
                .clone()
                .unwrap_or_else(|| "not recorded".into()),
        ),
        (
            "Library synced through",
            s.watermark
                .as_ref()
                .map(|w| w.chars().take(10).collect::<String>())
                .unwrap_or_else(|| "never".into()),
        ),
        ("Games in the cache", s.games.to_string()),
        ("Consoles", s.consoles.to_string()),
        ("Collections", s.collections.to_string()),
        (
            "Play recorded",
            if sessions == 0 {
                "none".to_owned()
            } else {
                format!(
                    "{games} games · {sessions} sessions · {}",
                    played_for(seconds)
                )
            },
        ),
    ];

    let (top, rest) = area.split_top(24.0);
    let spec = f.spec("WHAT THIS DEVICE HOLDS", 10.0);
    f.label(&spec, top, paint::FAINT);

    let table = rest.inset(Edges {
        top: 4.0,
        ..Edges::default()
    });
    for (i, (label, value)) in facts.iter().enumerate() {
        let line = Rect::new(table.x, table.y + i as f32 * 22.0, table.w.min(420.0), 20.0);
        let [name, val] =
            <[Rect; 2]>::try_from(line.row(10.0, &[Size::Fixed(150.0), Size::Grow(1.0)])).unwrap();
        let spec = f.spec(*label, size::LABEL);
        f.label(&spec, name, paint::FAINT);
        let spec = f.spec(value.as_str(), size::LABEL);
        f.label(&spec, val, paint::TEXT);
    }

    let note = Rect::new(
        table.x,
        table.y + facts.len() as f32 * 22.0 + 14.0,
        table.w.min(430.0),
        60.0,
    );
    let spec = f.wrapped(
        "Pulling the library and pushing saves is not wired up here yet. Sync from \
         the desktop app and this page will follow.",
        size::LABEL,
        note.w,
        3,
    );
    f.label(&spec, note, paint::DIM);
}

/// A plain list of name and count — the shape both Collections levels take.
fn draw_picker(f: &mut Frame, area: Rect, rows: &[(String, String)], at: usize, total: usize) {
    draw_rule_list(f, area, rows, at, total, None)
}

/// The Collections root: yours, a rule, then RomM's kinds.
///
/// The rule is drawn *between* rows rather than being one, so the cursor never
/// has to step over a thing that cannot be selected — which is the usual way a
/// divider in a list turns into a navigation bug.
fn draw_shelf(f: &mut Frame, area: Rect, rows: &[(String, String)], at: usize, mine: usize) {
    let rule = (mine > 0 && mine < rows.len()).then_some(mine);
    draw_rule_list(f, area, rows, at, rows.len(), rule)
}

fn draw_rule_list(
    f: &mut Frame,
    area: Rect,
    rows: &[(String, String)],
    at: usize,
    total: usize,
    rule_before: Option<usize>,
) {
    if rows.is_empty() {
        let spec = f.spec("Nothing here.", size::TITLE);
        f.label_centered(&spec, area, paint::DIM);
        return;
    }
    let step = size::ROW;
    // The rule's own space comes off the budget before the rows are counted.
    // Left out, the last row is pushed past the bottom and dropped — and when
    // that row is the cursor, the screen has no cursor on it at all.
    let room = area.h - rule_before.map_or(0.0, |_| size::GAP * 1.6);
    let fits = (room / step).floor().max(1.0) as usize;
    let first = at
        .saturating_sub(fits.saturating_sub(1))
        .min(total.saturating_sub(1));
    // The rule occupies space of its own, so everything below it shifts down
    // rather than the rule being drawn over the row that follows.
    let shift = |offset: usize| match rule_before {
        Some(r) if offset >= r && r > first => size::GAP * 1.6,
        _ => 0.0,
    };

    for (offset, (name, count)) in rows.iter().enumerate().skip(first).take(fits) {
        let line = Rect::new(
            area.x,
            area.y + (offset - first) as f32 * step + shift(offset),
            area.w,
            step,
        );
        if line.bottom() > area.bottom() {
            break;
        }
        if rule_before == Some(offset) && offset > first {
            f.fill(
                Rect::new(area.x, line.y - size::GAP * 0.8, area.w, 1.0),
                paint::RULE,
                0.0,
            );
        }
        f.hits.row(f.px(line), offset);
        let on = offset == at;
        f.hovering(offset, line, size::ROUND_SMALL);
        if on {
            f.pane(line, paint::CURSOR, size::ROUND_SMALL);
        }
        let inside = line.inset(Edges::xy(8.0, 5.0));
        let [title, n] =
            <[Rect; 2]>::try_from(inside.row(8.0, &[Size::Grow(1.0), Size::Fixed(60.0)])).unwrap();
        let spec = f.wrapped(name, size::LABEL, title.w, 1);
        f.label(&spec, title, if on { paint::TEXT } else { paint::DIM });
        let spec = f.spec(count.as_str(), 10.0);
        f.label_right(&spec, n, paint::FAINT);
    }
}

/// Seconds as something short enough for a column.
fn played_for(seconds: i64) -> String {
    match seconds {
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m", s / 60),
        s => format!("{}h {}m", s / 3600, (s % 3600) / 60),
    }
}

/// A keyboard with something in it, for judging the layout.
///
/// `ROMM_SDL_KEYBOARD="RetroAchievements token"` or `=secret:Wi-Fi password`.
/// Only a preview: the real one is opened by a settings field.
fn demo_keyboard() -> Option<keyboard::Keyboard> {
    let want = std::env::var("ROMM_SDL_KEYBOARD").ok()?;
    let (secret, prompt) = match want.strip_prefix("secret:") {
        Some(rest) => (true, rest.to_owned()),
        None => (false, want),
    };
    let mut kb = keyboard::Keyboard::new(prompt, if secret { "hunter2" } else { "frank" }, secret);
    kb.row = 1;
    kb.col = 3;
    Some(kb)
}

/// What to say the buttons do, for whatever is on screen.
fn hints_for(lib: &library::Library, typing: bool) -> &'static [(&'static str, &'static str)] {
    if typing {
        return &[("A", "type"), ("B", "delete"), ("L R", "page")];
    }
    if lib.capturing.is_some() {
        return &[("any", "bind"), ("B", "clear"), ("Start", "cancel")];
    }
    if lib.picking.is_some() {
        return &[("A", "choose"), ("B", "cancel")];
    }
    match lib.view {
        library::View::Options => &[
            ("A", "change"),
            ("< >", "slider"),
            ("B", "back"),
            ("L R", "tabs"),
        ],
        library::View::Roms => &[
            ("A", "play"),
            ("B", "back"),
            ("/", "search"),
            ("L R", "tabs"),
        ],
        _ => &[("A", "open"), ("B", "back"), ("L R", "tabs")],
    }
}

/// A sheet of options, over the settings screen.
///
/// The shape a mature settings menu uses: the choices are on screen at once and
/// the current one is marked, so choosing is reading and pressing rather than
/// holding a direction until the right value goes past.
fn draw_picker_sheet(f: &mut Frame, picker: &library::Picker, page: Rect) {
    f.fill(page, paint::SCRIM, 0.0);

    let step = size::ROW;
    // As many as the screen has room for, rather than a number picked in
    // advance. Nine was fine for the schemes and hid two of the eleven
    // backdrops — and a list you can only see the top of reads as a list that
    // is missing things.
    let head = 46.0 + size::GAP * 2.0;
    let fits = ((page.h - size::TABS - size::HEADER - size::HELP - head) / step)
        .floor()
        .max(3.0) as usize;
    let rows = picker.options.len().min(fits);
    let sheet_h = rows as f32 * step + head;
    let sheet = Rect::new(page.x, page.bottom() - sheet_h, page.w, sheet_h);
    f.fill(sheet, paint::SHEET, 0.0);

    let inner = sheet.inset(size::PAD);
    let (head_rect, list) = inner.split_top(24.0);
    let spec = f.spec(picker.title, size::TITLE);
    f.label(&spec, head_rect, paint::TEXT);

    // Keep the highlighted option on screen when the list is longer than the
    // sheet — the core map offers nine cores for some consoles.
    let first = picker.at.saturating_sub(rows.saturating_sub(1));
    if picker.options.len() > rows {
        let spec = f.spec(
            format!("{} of {}", picker.at + 1, picker.options.len()),
            10.0,
        );
        f.label_right(&spec, head_rect, paint::FAINT);
    }
    for (i, (_, label)) in picker.options.iter().enumerate().skip(first).take(rows) {
        let line = Rect::new(list.x, list.y + (i - first) as f32 * step, list.w, step);
        let on = i == picker.at;
        if on {
            f.pane(line, paint::CURSOR, size::ROUND_SMALL);
        }
        let inside = line.inset(Edges::xy(8.0, 5.0));
        let spec = f.wrapped(label, size::LABEL, inside.w, 1);
        f.label(&spec, inside, if on { paint::TEXT } else { paint::DIM });
    }
}

/// Waiting for a button, so a control can be put on it.
fn draw_capture(f: &mut Frame, cap: &library::Capture, page: Rect) {
    f.fill(page, paint::SCRIM, 0.0);
    let (top, rest) = page.inset(size::PAD).split_top(page.h * 0.42);
    let spec = f.spec(cap.label, 20.0);
    f.label_centered(&spec, top, paint::TEXT);
    let spec = f.wrapped(
        "Press the button you want. B clears it, Start leaves it alone.",
        size::TITLE,
        rest.w.min(380.0),
        3,
    );
    f.label_centered(&spec, Rect { h: 60.0, ..rest }, paint::DIM);
}

/// What the buttons do, along the bottom.
///
/// Permanent, the way a settings menu that expects a pad has it — a screen
/// whose controls you have to guess is one where left and right get pressed on
/// everything to find out what moves.
fn draw_help(f: &mut Frame, hints: &[(&str, &str)], area: Rect) {
    let mut x = area.x + size::GAP;
    for (button, what) in hints {
        let spec = f.spec(*button, 10.0);
        let (w, _) = f.painter.measure(f.gfx, &spec);
        let w = f.screen.scale.pt(w as f32);
        f.label(&spec, Rect { x, w, ..area }, paint::TEXT);
        x += w + 4.0;

        let spec = f.spec(*what, 10.0);
        let (w, _) = f.painter.measure(f.gfx, &spec);
        let w = f.screen.scale.pt(w as f32);
        f.label(&spec, Rect { x, w, ..area }, paint::FAINT);
        x += w + size::GAP * 1.6;
    }
}

/// The on-screen keyboard, over whatever asked for it.
///
/// Drawn as a sheet across the bottom two thirds rather than a dialog in the
/// middle: the field being filled has to stay visible while you type into it,
/// and on a 480-point screen a centered box leaves room for neither.
fn draw_keyboard(f: &mut Frame, kb: &keyboard::Keyboard, page: Rect) {
    // Block what is behind it. A keyboard you can read the library through is a
    // keyboard you keep losing the cursor on, and the page underneath is not
    // something you can act on while a field is open anyway.
    f.fill(page, paint::SCRIM, 0.0);

    // Laptop proportions: the key area is about two and a half times as wide as
    // it is tall, so keys come out wider than they are deep. Filling the height
    // instead gave 85-point rows — keys the size of postage stamps stacked into
    // a wall, which is nothing like a keyboard and read as one.
    let key_h = 34.0;
    let rows = keyboard::ROWS + 1;
    let keys_h = rows as f32 * key_h + size::GAP * (rows - 1) as f32;
    let sheet_h = keys_h + 52.0 + size::GAP * 2.0;
    let sheet = Rect::new(page.x, page.bottom() - sheet_h, page.w, sheet_h);
    f.fill(sheet, paint::SHEET, 0.0);

    let inner = sheet.inset(size::PAD);
    let (top, keys) = inner.split_top(52.0);

    // What is being asked for, and what has been typed so far.
    let prompt = f.spec(kb.prompt.as_str(), size::LABEL);
    f.label(&prompt, Rect { h: 15.0, ..top }, paint::FAINT);
    let field = Rect {
        y: top.y + 17.0,
        h: 26.0,
        ..top
    };
    f.pane(field, paint::CARD, size::ROUND_SMALL);
    let shown = kb.shown();
    let text = f.spec(
        if shown.is_empty() {
            "…"
        } else {
            shown.as_str()
        },
        size::TITLE,
    );
    f.label(
        &text,
        field.inset(Edges::xy(8.0, 5.0)),
        if shown.is_empty() {
            paint::FAINT
        } else {
            paint::TEXT
        },
    );

    let grid = kb.grid();
    let cell_w = (keys.w - size::GAP * (keyboard::COLS - 1) as f32) / keyboard::COLS as f32;
    let row_y = |row: usize| keys.y + row as f32 * (key_h + size::GAP);

    for row in 0..keyboard::ROWS {
        for col in 0..keyboard::COLS {
            let cell = Rect::new(
                keys.x + col as f32 * (cell_w + size::GAP),
                row_y(row),
                cell_w,
                key_h,
            );
            let on = !kb.on_actions() && kb.row == row && kb.col == col;
            f.pane(
                cell,
                if on { paint::CURSOR } else { paint::CARD },
                size::ROUND_SMALL,
            );
            let ch: String = grid[row].chars().nth(col).into_iter().collect();
            let spec = f.spec(ch.as_str(), size::TITLE);
            f.label_centered(&spec, cell, if on { paint::TEXT } else { paint::DIM });
        }
    }

    // The action row. Space is drawn wide because that is what a thumb expects,
    // and stays one cell to the cursor.
    let weights = [1.4, 1.2, 3.4, 1.4, 1.6];
    let total: f32 = weights.iter().sum();
    let usable = keys.w - size::GAP * (weights.len() - 1) as f32;
    let mut x = keys.x;
    for (i, action) in keyboard::ACTIONS.iter().enumerate() {
        let w = usable * weights[i] / total;
        let cell = Rect::new(x, row_y(keyboard::ROWS), w, key_h);
        let on = kb.action() == Some(*action);
        f.pane(
            cell,
            if on { paint::CURSOR } else { paint::CARD },
            size::ROUND_SMALL,
        );
        let spec = f.spec(action.label(), size::LABEL);
        f.label_centered(&spec, cell, if on { paint::TEXT } else { paint::DIM });
        x += w + size::GAP;
    }
}

/// The Settings tab: the panes, then one pane's settings.
///
/// Two levels, like Collections. The settings themselves are a list with the
/// value on the right and one sentence of explanation beside it — because a
/// setting whose name you have to guess the meaning of is a setting nobody
/// touches, and on a handheld there is no tooltip to hover for.
fn draw_settings(f: &mut Frame, lib: &mut library::Library, area: Rect) {
    if lib.view == library::View::Wifi {
        let (top, list) = area.split_top(22.0);
        let spec = f.spec(lib.wifi.note().as_str(), size::LABEL);
        f.label(&spec, top, paint::FAINT);
        let names = lib.wifi.names();
        let joined = match &lib.wifi {
            wifi::State::Networks { joined, .. } => joined.clone(),
            _ => None,
        };
        let rows: Vec<_> = names
            .iter()
            .map(|n| {
                let mark = if joined.as_deref() == Some(n.as_str()) {
                    "saved"
                } else {
                    ""
                };
                (n.clone(), mark.to_owned())
            })
            .collect();
        draw_picker(f, list, &rows, lib.wifi_at, rows.len());
        return;
    }

    if lib.view == library::View::Panes {
        let rows: Vec<_> = lib
            .panes
            .iter()
            .map(|p| (p.label.to_owned(), format!("{}", p.entries.len())))
            .collect();
        draw_picker(f, area, &rows, lib.pane_at, rows.len());
        return;
    }

    let Some(pane) = lib.panes.get(lib.pane_at) else {
        return;
    };
    let [list, aside] =
        <[Rect; 2]>::try_from(area.row(size::GAP, &[Size::Grow(1.0), Size::Fixed(size::ASIDE)]))
            .unwrap();

    let step = size::ROW;
    let fits = (list.h / step).floor().max(1.0) as usize;
    let first = lib.option_at.saturating_sub(fits.saturating_sub(1));

    for (offset, entry) in pane.entries.iter().enumerate().skip(first).take(fits) {
        let line = Rect::new(
            list.x,
            list.y + (offset - first) as f32 * step,
            list.w,
            step,
        );
        if line.bottom() > list.bottom() {
            break;
        }
        f.hits.row(f.px(line), offset);
        let on = offset == lib.option_at;
        f.hovering(offset, line, size::ROUND_SMALL);
        if on {
            f.pane(line, paint::CURSOR, size::ROUND_SMALL);
        }
        let inside = line.inset(Edges::xy(8.0, 5.0));
        let [name, value] =
            <[Rect; 2]>::try_from(inside.row(8.0, &[Size::Grow(1.0), Size::Fixed(110.0)])).unwrap();
        let spec = f.wrapped(entry.label, size::LABEL, name.w, 1);
        f.label(&spec, name, if on { paint::TEXT } else { paint::DIM });

        // Only a slider shows arrows, because only a slider is changed with
        // them. A toggle flips on A and a choice opens a list, and drawing
        // arrows on those is how the screen taught the wrong control.
        let sliding = entry.steps();
        let shown = if on && sliding {
            format!("\u{2039} {} \u{203a}", entry.value())
        } else {
            entry.value()
        };
        let spec = f.spec(shown.as_str(), 11.0);
        f.label_right(
            &spec,
            value,
            match &entry.kind {
                settings::Kind::ReadOnly(_) => paint::FAINT,
                _ if on => paint::TEXT,
                _ => paint::DIM,
            },
        );
    }

    // What the highlighted setting does.
    f.pane(aside, paint::COLUMN, size::ROUND);
    if let Some(entry) = pane.entries.get(lib.option_at) {
        let inner = aside.inset(size::PAD);
        let title = f.wrapped(entry.label, size::TITLE, inner.w, 2);
        let used = f.label(&title, inner, paint::TEXT);
        let body = Rect {
            y: inner.y + used + 8.0,
            h: inner.h - used - 8.0,
            ..inner
        };
        let help = f.wrapped(entry.help, size::LABEL, body.w, 8);
        f.label(&help, body, paint::DIM);
    }
}

/// A tab that is in the row but has nothing behind it yet.
///
/// It says which one and that it is coming, rather than drawing an empty
/// frame. The shoulder buttons cycle every tab whether or not it is built, so
/// landing here has to be legible — an empty page reads as a crash.
fn draw_unbuilt(f: &mut Frame, id: &str, area: Rect) {
    let (what, why) = match id {
        "mine" => (
            "Collections",
            "Your RomM collections. The data is already in the cache; this page is not built yet.",
        ),
        "history" => ("History", "Everything played, and when. Not built yet."),
        "syncing" => (
            "Syncing",
            "Pulling the library and pushing saves. Nothing here yet — this is the one that has to be written from scratch.",
        ),
        "settings" => (
            "Settings",
            "Bindings, artwork and the library path. Not built yet.",
        ),
        _ => ("Not built", "Nothing here yet."),
    };
    // Title and sentence as one block near the middle, not one at a third of
    // the way down and the other at two thirds with the page between them.
    let inner = area.inset(size::PAD);
    let (top, rest) = inner.split_top(inner.h * 0.46);
    let spec = f.spec(what, 20.0);
    f.label_centered(&spec, top, paint::DIM);
    let spec = f.wrapped(why, size::LABEL, rest.w.min(330.0), 4);
    f.label_centered(&spec, Rect { h: 54.0, ..rest }, paint::FAINT);
}

/// The tab row and the header, across the top.
fn draw_chrome(
    f: &mut Frame,
    lib: &library::Library,
    tabs: Rect,
    header: Rect,
    status: &status::Status,
) {
    f.pane(
        Rect::new(tabs.x, tabs.y, tabs.w, tabs.h + header.h),
        paint::BAR,
        0.0,
    );

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
            f.fill(
                Rect::new(slot.x, slot.bottom() - 3.0, slot.w, 3.0),
                paint::CURSOR,
                2.0,
            );
        }
        f.label_centered(
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

    // The corner every device has: clock, then signal, then charge. Laid out
    // from the right edge inwards so the clock keeps its place as the other two
    // appear and disappear — a time that shuffles sideways when the charger
    // goes in is the sort of thing you notice every time.
    let mut right = tabs.right() - size::GAP;
    for part in status.parts().iter().rev() {
        let spec = f.spec(part.as_str(), 10.0);
        let (w, _) = f.painter.measure(f.gfx, &spec);
        let w = f.screen.scale.pt(w as f32);
        let slot = Rect::new(right - w, tabs.y, w, tabs.h);
        f.label_centered(&spec, slot, paint::DIM);
        right = slot.x - size::GAP;
    }

    // Where you are on the left, how the list is arranged on the right.
    let inner = header.inset(Edges::xy(size::GAP, 0.0));
    // What the line under the tabs says. One useful fact per screen, not a
    // count of the furniture — "8 groups" told you how the settings happened to
    // be filed, which is not something anybody wants to know.
    let here = match (lib.section().id, lib.view) {
        // Left as it was: Frank said this one already reads right.
        ("library", library::View::Platforms) => format!("{} consoles", lib.consoles.len()),
        ("library", library::View::Roms) => match lib.console() {
            Some(c) if !lib.query.is_empty() => {
                format!(
                    "{} — {} matching \u{201c}{}\u{201d}",
                    c.name,
                    lib.shown(),
                    lib.query
                )
            }
            Some(c) => format!("{} — {} games", c.name, lib.shown()),
            None => String::new(),
        },
        ("library", library::View::Scripts) => match lib.console() {
            Some(c) => format!("{} — {}", c.name, lib.ports.len()),
            None => String::new(),
        },
        ("mine", library::View::Groups) => match (lib.mine_count, lib.shelves.len()) {
            (0, n) => format!("{n} from RomM"),
            (n, all) if n == all => format!("{n} you made"),
            (n, all) => format!("{n} you made · {} from RomM", all - n),
        },
        ("mine", library::View::Collections) => format!("{} collections", lib.cols.len()),
        ("mine", _) => format!("{} — {} games", lib.col_name, lib.shown()),
        ("history", _) => {
            let (games, secs) = (lib.sync.games_played, lib.sync.seconds_played);
            format!("{games} games · {}", played_for(secs))
        }
        ("settings", library::View::Panes) => "How this device behaves".to_owned(),
        ("settings", library::View::Wifi) => "Wi-Fi".to_owned(),
        ("settings", _) => lib
            .panes
            .get(lib.pane_at)
            .map(|p| p.label.to_owned())
            .unwrap_or_default(),
        (_, _) => lib.section().label.to_owned(),
    };
    let title = f.spec(here, size::TITLE);
    let (_, th) = f.painter.measure(f.gfx, &title);
    let centered =
        Rect::new(inner.x, inner.y, inner.w, inner.h).center(inner.w, f.screen.scale.pt(th as f32));
    f.label(
        &title,
        Rect {
            x: inner.x,
            ..centered
        },
        paint::TEXT,
    );

    // How the games are ordered, when there are games. No grid/list control:
    // on this panel it is a list, and a button offering the arrangement that
    // does not fit is a button that makes the screen worse.
    if lib.view == library::View::Roms {
        let filters = lib.filters();
        let arranged = if filters.is_empty() {
            lib.order_label().to_owned()
        } else {
            format!("{}  ·  {}", lib.order_label(), filters.join(" + "))
        };
        let spec = f.spec(arranged, 11.0);
        f.label_right(&spec, centered, paint::DIM);
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
            f.hovering(offset, slot, size::ROUND_SMALL);
            if on {
                match focused {
                    true => f.fill(slot, paint::CURSOR, size::ROUND_SMALL),
                    false => f.pane(slot, paint::CARD, size::ROUND_SMALL),
                }
            }
            let [name, count] = <[Rect; 2]>::try_from(
                slot.inset(Edges::xy(8.0, 5.0))
                    .row(6.0, &[Size::Grow(1.0), Size::Fixed(46.0)]),
            )
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
    // Whole rows only. A part-row looked like more to scroll to and was
    // really a row with its captions cut in half.
    let rows = (area.h / (tile_h + size::GAP)).floor().max(1.0) as usize;
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
        f.hovering(offset, tile, size::ROUND);
        if on {
            f.outline(tile, 2.0, paint::CURSOR, size::ROUND);
        }

        let inner = tile.inset(Edges::all(10.0));
        let (art, caption) = inner.split_top(size::TILE_ART - 10.0);
        f.console_art(&console.slug, art, 0.0);

        let name = f.wrapped(&console.name, size::LABEL, caption.w, 2);
        let used = f.label(&name, caption, if on { paint::TEXT } else { paint::DIM });
        let under = Rect {
            y: caption.y + used + 4.0,
            h: 14.0,
            ..caption
        };
        let [dot, count] =
            <[Rect; 2]>::try_from(under.row(4.0, &[Size::Fixed(9.0), Size::Grow(1.0)])).unwrap();
        let spec = f.spec("●", 9.0);
        f.label(&spec, dot, paint::HERE);
        let spec = f.spec(format!("{} games", console.games), 11.0);
        f.label(&spec, count, paint::FAINT);
    }
}

/// One console's games.
fn draw_game_list(f: &mut Frame, lib: &mut library::Library, area: Rect) {
    let step = size::ROW;
    lib.relayout(1);

    let (shown, at, was) = (lib.shown(), lib.at, lib.scrolled);
    let ask = |top: f32| rowwindow::Ask::new(shown, 1, step, top, area.h);
    let top = rowwindow::scroll_to(at, ask(was)).unwrap_or(was);
    lib.scrolled = top;
    let band = rowwindow::band(ask(top));

    let rows: Vec<_> = lib
        .showing()
        .enumerate()
        .skip(band.first)
        .take(band.count)
        .map(|(i, (r, _))| (i, r.name.clone(), r.favorite, r.downloaded, r.size_bytes))
        .collect();

    for (i, name, favorite, downloaded, bytes) in rows {
        let line = Rect::new(area.x, area.y + i as f32 * step - top, area.w, step);
        if line.bottom() < area.y || line.bottom() > area.bottom() {
            continue;
        }
        f.hits.row(f.px(line), i);
        let on = i == lib.at;
        f.hovering(i, line, size::ROUND_SMALL);
        if on {
            f.pane(line, paint::CURSOR, size::ROUND_SMALL);
        }

        // No console column. Every row in this list is the console named in
        // the header, so it said the same word ninety-four times down the page
        // and took ninety points the titles needed — "Alex Kidd in the..." was
        // being cut off to print "megadrive" beside it.
        let inside = line.inset(Edges::xy(8.0, 5.0));
        let [mark, title, size] = <[Rect; 3]>::try_from(inside.row(
            8.0,
            &[Size::Fixed(12.0), Size::Grow(1.0), Size::Fixed(66.0)],
        ))
        .unwrap();

        let spec = f.spec(if downloaded { "\u{25cf}" } else { "\u{25cb}" }, 11.0);
        f.label(
            &spec,
            mark,
            if downloaded { paint::HERE } else { paint::AWAY },
        );

        // The star sits before the name rather than beside it, so a starred
        // game and a plain one line their names up anyway.
        let mut left = title;
        if favorite {
            let spec = f.spec("\u{2605}", 11.0);
            let (w, _) = f.painter.measure(f.gfx, &spec);
            let w = f.screen.scale.pt(w as f32) + 5.0;
            f.label(&spec, left, paint::STAR);
            left = Rect {
                x: left.x + w,
                w: left.w - w,
                ..left
            };
        }
        let spec = f.wrapped(&name, size::LABEL, left.w, 1);
        f.label(&spec, left, if on { paint::TEXT } else { paint::DIM });

        let spec = f.spec(romm_desktop::util::human(bytes.max(0) as u64), 11.0);
        f.label_right(&spec, size, paint::DIM);
    }
}

/// The preview column: the cover, the name, and what is known about it.
fn draw_detail(f: &mut Frame, lib: &library::Library, area: Rect) {
    let Some(detail) = lib.detail() else { return };
    let (art, rest) = area.split_top(area.w / lib.aspect.clamp(0.3, 3.0));
    // The game's own cover if there is one, and the console's picture if not —
    // an empty pane at the top of the column reads as a broken panel.
    if !f.cover(
        detail.id,
        &detail.platform,
        &detail.stem,
        art,
        size::ROUND_SMALL,
    ) {
        f.console_art(&detail.platform, art, size::ROUND_SMALL);
    }

    // The one place a title is not cut short: the card had to fit it into 150
    // points and this column is where the whole thing goes.
    let below = rest.inset(Edges {
        top: size::GAP,
        ..Edges::default()
    });
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
