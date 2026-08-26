//! moose-patch — the addon.
//!
//! Two tabs, a list of dials, and one deliberate moment where everything you
//! turned actually happens. See `docs/knulli-addon.md` for what it is for and
//! `model` for why nothing applies as you touch it.
//!
//! It also runs without a window:
//!
//!     moose-patch --status     what every patch is currently at
//!     moose-patch --restore    put the device back to the saved profile
//!
//! `--restore` is what a freshly installed KNULLI needs, and it is deliberately
//! reachable over ssh — a device that has just been reflashed has no way to
//! launch a windowed app until the patches that make that possible are on.

use anyhow::{Context, Result};
use moose_patch::model::{App, Kind, Overlay, Tab};
use moose_patch::patch::{Patch, Paths, State};
use moose_patch::{catalogue, profile, rows, ui};
use romm_sdl::gfx::Gfx;
use romm_sdl::input;
use romm_sdl::text;
use sdl2::controller::Button;
use sdl2::event::Event;
use sdl2::keyboard::Keycode;

/// What the pad and the keyboard both turn into, so the loop below reads the
/// same whichever one you are holding.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum Press {
    Up,
    Down,
    Left,
    Right,
    Accept,
    Back,
    Detail,
    TabLeft,
    TabRight,
    Quit,
}

fn from_key(key: Keycode) -> Option<Press> {
    Some(match key {
        Keycode::Up | Keycode::W => Press::Up,
        Keycode::Down | Keycode::S => Press::Down,
        Keycode::Left | Keycode::A => Press::Left,
        Keycode::Right | Keycode::D => Press::Right,
        Keycode::Return | Keycode::Space => Press::Accept,
        Keycode::Backspace => Press::Back,
        Keycode::X => Press::Detail,
        Keycode::Q => Press::TabLeft,
        Keycode::E => Press::TabRight,
        Keycode::Escape => Press::Quit,
        _ => return None,
    })
}

/// SDL names its buttons after the Xbox layout, so `Button::A` is the bottom
/// one — which is what this device calls A too.
fn from_button(button: Button) -> Option<Press> {
    Some(match button {
        Button::DPadUp => Press::Up,
        Button::DPadDown => Press::Down,
        Button::DPadLeft => Press::Left,
        Button::DPadRight => Press::Right,
        Button::A => Press::Accept,
        Button::B => Press::Back,
        Button::X | Button::Y => Press::Detail,
        Button::LeftShoulder => Press::TabLeft,
        Button::RightShoulder => Press::TabRight,
        _ => return None,
    })
}

fn open_window(video: &sdl2::VideoSubsystem) -> Result<sdl2::video::Window, String> {
    // The device's screen exactly. On a desktop it is a small window, which is
    // the point: the last front end was drawn at four times the size and every
    // judgement made about it was wrong.
    let (w, h) = (ui::PANEL.0, ui::PANEL.1);
    let mut builder = video.window("moose-patch", w, h);
    if cfg!(any(target_os = "linux", target_os = "android")) {
        builder.fullscreen_desktop();
    }
    builder
        .position_centered()
        .opengl()
        .build()
        .map_err(|e| e.to_string())
}

fn main() -> Result<()> {
    romm_desktop::datadir::anchor();
    let paths = Paths::default();
    let patches = catalogue::all(&paths);

    match std::env::args().nth(1).as_deref() {
        Some("--status") => return status(&patches),
        Some("--restore") => return restore(&paths, &patches),
        Some("--save") => {
            profile::save(&paths, &patches)?;
            println!("wrote {}", paths.profile().display());
            return Ok(());
        }
        Some(other) if other.starts_with("--") => {
            eprintln!("moose-patch [--status | --restore | --save]");
            std::process::exit(2);
        }
        _ => {}
    }
    window(&paths, &patches)
}

fn status(patches: &[Patch]) -> Result<()> {
    for patch in patches {
        let at = match patch.state() {
            State::At(i) => patch.choices[i].name.clone(),
            State::Changed => "changed — not at any known setting".into(),
        };
        println!("{:<16} {at}", patch.id);
    }
    Ok(())
}

fn restore(paths: &Paths, patches: &[Patch]) -> Result<()> {
    let done = profile::restore(paths, patches)?;
    for line in &done.applied {
        println!("applied  {line}");
    }
    for id in &done.already {
        println!("already  {id}");
    }
    for line in &done.failed {
        eprintln!("failed   {line}");
    }
    for id in &done.unknown {
        // Not an error that stops the rest: a profile from a newer build
        // should still restore everything this one understands.
        eprintln!("unknown  {id}");
    }
    if done.applied.is_empty() && done.already.is_empty() && done.unknown.is_empty() {
        eprintln!("nothing to restore — no profile at {}", paths.profile().display());
    }
    if !done.failed.is_empty() {
        std::process::exit(1);
    }
    Ok(())
}

fn window(paths: &Paths, patches: &[Patch]) -> Result<()> {
    let sdl = sdl2::init()
        .map_err(anyhow::Error::msg)
        .context("starting SDL")?;
    let video = sdl
        .video()
        .map_err(anyhow::Error::msg)
        .context("opening the display")?;
    {
        // Fixed before the window is made; SDL will not change them after.
        let attr = video.gl_attr();
        if cfg!(any(target_os = "android", target_os = "linux")) {
            attr.set_context_profile(sdl2::video::GLProfile::GLES);
            attr.set_context_version(3, 0);
        } else {
            attr.set_context_profile(sdl2::video::GLProfile::Core);
            attr.set_context_version(3, 3);
        }
    }

    let win = open_window(&video)
        .map_err(anyhow::Error::msg)
        .context("opening a window")?;
    let _context = win
        .gl_create_context()
        .map_err(anyhow::Error::msg)
        .context("creating an OpenGL context")?;
    win.gl_set_context_to_current().map_err(anyhow::Error::msg)?;

    let mut gfx = unsafe { Gfx::new(&video) }.context("setting up the renderer")?;
    let _ = video.gl_set_swap_interval(sdl2::video::SwapInterval::VSync);
    let mut painter = text::Painter::new().context("finding fonts")?;

    // The pad, through the same code the front end used — the GUID on this
    // family of handhelds is shared by four different devices, so the mapping
    // has to come from the OS's own es_input.cfg rather than SDL's database.
    let controllers = sdl.game_controller().map_err(anyhow::Error::msg)?;
    let joysticks = sdl.joystick().map_err(anyhow::Error::msg)?;
    let _pads = input::Pads::open_first(&controllers, &joysticks);

    let mut app = App {
        tab: Tab::Patches,
        sync: rows::sync(None, None),
        patches: rows::patches(patches),
        overlay: Overlay::None,
        should_quit: false,
    };
    let mut view = ui::Ui::default();
    let mut events = sdl.event_pump().map_err(anyhow::Error::msg)?;

    while !app.should_quit {
        // A menu has nothing moving in it, so it waits for something to happen
        // rather than burning a core drawing the same frame. On a handheld
        // that is the difference between warm and not.
        let woke = events.wait_event_timeout(250);
        let arrived: Vec<Event> = woke.into_iter().chain(events.poll_iter()).collect();

        for event in arrived {
            let press = match event {
                Event::Quit { .. } => Some(Press::Quit),
                Event::KeyDown { keycode: Some(key), repeat: false, .. } => from_key(key),
                Event::ControllerButtonDown { button, .. } => from_button(button),
                _ => None,
            };
            let Some(press) = press else { continue };
            act(&mut app, &mut view, press, paths, patches);
        }

        gfx.resize(ui::PANEL.0 as f32, ui::PANEL.1 as f32);
        view.draw(&gfx, &mut painter, &app);
        win.gl_swap_window();
    }
    Ok(())
}

/// Carry out the queue. Everything that failed is left showing as still
/// pending, which is the honest thing for the menu to say afterwards.
fn run_queue(app: &mut App, paths: &Paths, patches: &[Patch]) {
    for (id, index) in app.orders() {
        let Some(patch) = patches.iter().find(|p| p.id == id) else { continue };
        match patch.apply(index) {
            Ok(()) => {
                for page in [&mut app.sync, &mut app.patches] {
                    if let Some(row) = page.rows.iter_mut().find(|r| r.id == id) {
                        row.settle();
                    }
                }
            }
            Err(e) => eprintln!("{id}: {e:#}"),
        }
    }
    if let Err(e) = profile::save(paths, patches) {
        eprintln!("could not write the profile: {e:#}");
    }
}

/// One press. Split out so the whole of the app's behaviour can be exercised
/// without a window — see the tests at the foot of this file.
fn act(app: &mut App, view: &mut ui::Ui, press: Press, paths: &Paths, patches: &[Patch]) {
    if press == Press::Quit {
        app.should_quit = true;
        return;
    }
    match app.overlay {
        Overlay::Applying { .. } => {}

        Overlay::Detail => {
            if matches!(press, Press::Back | Press::Accept | Press::Detail) {
                app.overlay = Overlay::None;
            }
        }

        Overlay::ConfirmApply => match press {
            Press::Accept => {
                run_queue(app, paths, patches);
                app.overlay = Overlay::None;
            }
            Press::Back => app.overlay = Overlay::None,
            _ => {}
        },

        Overlay::ConfirmDiscard => match press {
            Press::Accept => {
                app.sync.revert_all();
                app.patches.revert_all();
                app.should_quit = true;
            }
            Press::Back => app.overlay = Overlay::None,
            _ => {}
        },

        Overlay::None => match press {
            Press::Up => app.page_mut().step(-1),
            Press::Down => app.page_mut().step(1),
            Press::Left => app.page_mut().turn_selected(-1),
            Press::Right => app.page_mut().turn_selected(1),
            Press::TabLeft => {
                app.next_tab(-1);
                view.first = 0;
            }
            Press::TabRight => {
                app.next_tab(1);
                view.first = 0;
            }
            Press::Detail => {
                if app.page().selected().is_some_and(|r| !r.detail.is_empty()) {
                    app.overlay = Overlay::Detail;
                }
            }
            Press::Accept => {
                if !app.queue().is_empty() {
                    app.overlay = Overlay::ConfirmApply;
                } else if matches!(
                    app.page().selected().map(|r| &r.kind),
                    Some(Kind::Action { .. })
                ) {
                    // Push, pull, take offline. Nothing to reconcile against,
                    // so these run on their own rather than joining the queue.
                    // The core already has the conflict handling they need —
                    // `romm_desktop::savesync` — and wiring them up is next.
                }
            }
            Press::Back => {
                if app.queue().is_empty() {
                    app.should_quit = true;
                } else {
                    app.overlay = Overlay::ConfirmDiscard;
                }
            }
            Press::Quit => app.should_quit = true,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A whole app pointed at a temporary directory, so pressing buttons in
    /// these tests really does write files and read them back.
    fn fixture(name: &str) -> (App, ui::Ui, Paths, Vec<Patch>) {
        let dir = std::env::temp_dir().join(format!("moose-main-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let paths = Paths::new(dir);
        let patches = catalogue::all(&paths);
        let app = App {
            tab: Tab::Patches,
            sync: rows::sync(None, None),
            patches: rows::patches(&patches),
            overlay: Overlay::None,
            should_quit: false,
        };
        (app, ui::Ui::default(), paths, patches)
    }

    fn press(f: &mut (App, ui::Ui, Paths, Vec<Patch>), p: Press) {
        act(&mut f.0, &mut f.1, p, &f.2, &f.3);
    }

    #[test]
    fn b_leaves_straight_away_when_nothing_is_queued() {
        let mut f = fixture("leave");
        press(&mut f, Press::Back);
        assert!(f.0.should_quit);
    }

    #[test]
    fn b_asks_first_when_something_is_queued() {
        // The whole point of queuing: work in hand must not vanish because a
        // thumb found the wrong button.
        let mut f = fixture("ask");
        press(&mut f, Press::Right);
        press(&mut f, Press::Back);
        assert_eq!(f.0.overlay, Overlay::ConfirmDiscard);
        assert!(!f.0.should_quit);
    }

    #[test]
    fn a_never_applies_without_confirming() {
        let mut f = fixture("confirm");
        press(&mut f, Press::Right);
        press(&mut f, Press::Accept);
        assert_eq!(f.0.overlay, Overlay::ConfirmApply);
        assert_eq!(f.0.queue().len(), 1, "nothing may be applied yet");
        // And nothing has been written.
        assert!(!f.2.knulli_conf().exists());
    }

    #[test]
    fn confirming_writes_the_files_and_empties_the_queue() {
        // End to end: a press of A really does put the block in knulli.conf.
        let mut f = fixture("apply");
        press(&mut f, Press::Right);
        press(&mut f, Press::Accept);
        press(&mut f, Press::Accept);
        assert!(f.0.queue().is_empty());
        let conf = std::fs::read_to_string(f.2.knulli_conf()).unwrap();
        assert!(conf.contains("## moose-patch: hotkeys"), "{conf}");
        assert_eq!(f.3[0].state(), State::At(1));
    }

    #[test]
    fn cancelling_keeps_the_queue_and_writes_nothing() {
        let mut f = fixture("cancel");
        press(&mut f, Press::Right);
        press(&mut f, Press::Accept);
        press(&mut f, Press::Back);
        assert_eq!(f.0.queue().len(), 1);
        assert!(!f.2.knulli_conf().exists());
    }

    #[test]
    fn applying_writes_a_profile_that_can_rebuild_the_device() {
        let mut f = fixture("profile");
        press(&mut f, Press::Right);
        press(&mut f, Press::Accept);
        press(&mut f, Press::Accept);
        let saved = std::fs::read_to_string(f.2.profile()).unwrap();
        assert!(saved.contains("hotkeys = \"ON\""), "{saved}");
    }

    #[test]
    fn discarding_puts_every_dial_back() {
        let mut f = fixture("discard");
        press(&mut f, Press::Right);
        press(&mut f, Press::Down);
        press(&mut f, Press::Right);
        assert_eq!(f.0.queue().len(), 2);
        press(&mut f, Press::Back);
        press(&mut f, Press::Accept);
        assert!(f.0.queue().is_empty());
        assert!(f.0.should_quit);
    }

    #[test]
    fn the_queue_survives_changing_tabs() {
        // Turning a dial on one tab and wandering to the other must not
        // quietly drop it — the counter says "not applied", and it has to
        // mean it.
        let mut f = fixture("tabs");
        press(&mut f, Press::Right);
        press(&mut f, Press::TabLeft);
        assert_eq!(f.0.tab, Tab::Sync);
        assert_eq!(f.0.queue().len(), 1);
    }

    #[test]
    fn x_opens_the_detail_and_b_closes_it() {
        let mut f = fixture("detail");
        press(&mut f, Press::Detail);
        assert_eq!(f.0.overlay, Overlay::Detail);
        press(&mut f, Press::Back);
        assert_eq!(f.0.overlay, Overlay::None);
        assert!(!f.0.should_quit, "closing a panel is not leaving the app");
    }

    #[test]
    fn dials_do_nothing_while_a_panel_is_up() {
        let mut f = fixture("behind");
        press(&mut f, Press::Detail);
        press(&mut f, Press::Right);
        assert!(f.0.queue().is_empty(), "the list is not live behind a panel");
    }

    #[test]
    fn a_second_visit_opens_at_what_the_first_one_did() {
        // Apply, then rebuild the menu the way starting the app again would.
        // The row has to come back already on, or every session proposes the
        // same change forever.
        let mut f = fixture("again");
        press(&mut f, Press::Right);
        press(&mut f, Press::Accept);
        press(&mut f, Press::Accept);

        let fresh = rows::patches(&catalogue::all(&f.2));
        let row = fresh.rows.iter().find(|r| r.id == "hotkeys").unwrap();
        assert_eq!(row.value(), "ON");
        assert!(!row.pending());
    }
}
