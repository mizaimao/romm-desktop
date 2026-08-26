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

/// Raw joystick numbers, for a pad SDL could not describe.
///
/// Only consulted when no controller opened. The numbering is the one
/// `es_input.cfg` gives this family of handhelds — 0 is the button printed A,
/// 1 the one printed B — which is also what RetroArch is configured with.
fn from_joy_button(index: u8, inverted: bool) -> Option<Press> {
    let (accept, back) = if inverted { (1, 0) } else { (0, 1) };
    Some(match index {
        i if i == accept => Press::Accept,
        i if i == back => Press::Back,
        2 | 3 => Press::Detail,
        4 => Press::TabLeft,
        5 => Press::TabRight,
        _ => return None,
    })
}

fn from_hat(state: sdl2::joystick::HatState) -> Option<Press> {
    use sdl2::joystick::HatState as H;
    Some(match state {
        H::Up => Press::Up,
        H::Down => Press::Down,
        H::Left => Press::Left,
        H::Right => Press::Right,
        _ => return None,
    })
}

/// Whether this handheld's face buttons are the other way round.
///
/// SDL names buttons after where they sit on an Xbox pad, so `Button::A` is the
/// *bottom* one. This device is laid out like a Nintendo pad, where the button
/// printed **A** is on the right — so SDL calls it `B`. EmulationStation knows
/// this and carries `InvertButtons` for it; reading the same setting means the
/// app confirms with the same button the rest of the device confirms with,
/// rather than with whichever one SDL happens to have named first.
///
/// This was not a guess. The press log showed four presses arriving as `Back`
/// where apply was meant, and one earlier session where `Back` then `Accept`
/// opened the discard prompt and took it.
fn buttons_inverted(paths: &Paths) -> bool {
    std::fs::read_to_string(paths.es_settings())
        .unwrap_or_default()
        .lines()
        .find(|line| line.contains("name=\"InvertButtons\""))
        .map(|line| line.contains("value=\"true\""))
        .unwrap_or(false)
}

fn from_button(button: Button, inverted: bool) -> Option<Press> {
    let (accept, back) = if inverted {
        (Button::B, Button::A)
    } else {
        (Button::A, Button::B)
    };
    Some(match button {
        Button::DPadUp => Press::Up,
        Button::DPadDown => Press::Down,
        Button::DPadLeft => Press::Left,
        Button::DPadRight => Press::Right,
        b if b == accept => Press::Accept,
        b if b == back => Press::Back,
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
    // Not `fullscreen_desktop`. On this handheld SDL's kmsdrm backend owns the
    // whole screen anyway, and asking for a fullscreen mode change on top of
    // that is a mode this app has to hand back cleanly when it exits — which
    // is one more thing to get wrong between the app quitting and
    // EmulationStation drawing again.
    let mut builder = video.window("moose-patch", w, h);
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
    // The server first, because "it cannot reach RomM" and "no patch is on"
    // are different problems and this is the one line that tells them apart.
    let cfg = romm_desktop::config::Config::load().unwrap_or_default();
    if cfg.server.url.is_empty() {
        println!("{:<16} not configured", "server");
    } else {
        let auth = if cfg.server.token.is_some() {
            "token"
        } else if !cfg.server.password.is_empty() {
            "password"
        } else {
            "no credential"
        };
        println!("{:<16} {} ({auth})", "server", cfg.server.url);
    }
    println!();
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
    // Which button confirms. See `buttons_inverted`.
    let inverted = buttons_inverted(paths);
    println!("face buttons inverted: {inverted}");

    let controllers = sdl.game_controller().map_err(anyhow::Error::msg)?;
    let joysticks = sdl.joystick().map_err(anyhow::Error::msg)?;
    let pads = input::Pads::open_first(&controllers, &joysticks);
    // A pad SDL cannot describe sends joystick events and no controller ones.
    // Listening only for controller events would look exactly like the app
    // having frozen: it draws, and nothing you press does anything.
    let raw_pad = !pads.is_open();
    if raw_pad {
        eprintln!("no controller mapping — falling back to raw joystick buttons");
        for index in 0..joysticks.num_joysticks().unwrap_or(0) {
            let _ = joysticks.open(index);
        }
    }

    // The server, if this device has been given credentials. Read once: the
    // sync tab shows where it would talk to, and says so plainly when nowhere.
    let cfg = romm_desktop::config::Config::load().unwrap_or_default();
    let server = (!cfg.server.url.is_empty()).then(|| cfg.server.url.clone());

    let mut app = App {
        tab: Tab::Patches,
        sync: rows::sync(server.as_deref(), None),
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
                Event::ControllerButtonDown { button, .. } => from_button(button, inverted),
                Event::JoyButtonDown { button_idx, .. } if raw_pad => {
                    from_joy_button(button_idx, inverted)
                }
                Event::JoyHatMotion { state, .. } if raw_pad => from_hat(state),
                _ => None,
            };
            let Some(press) = press else { continue };
            // Every press, in the log. When a button did nothing, the question
            // is always whether the app never saw it or saw it and ignored it,
            // and one line settles that without a second trip to the device.
            eprintln!("press {press:?}");
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

        Overlay::ConfirmAction { .. } => match press {
            Press::Accept => {
                // Push, pull, take offline. `romm_desktop::savesync` already
                // has the conflict handling these need; wiring them up is the
                // next piece of work, and until then this must not pretend.
                let id = app.page().selected().map(|r| r.id.clone()).unwrap_or_default();
                eprintln!("sync action '{id}' is not wired up yet");
                app.overlay = Overlay::None;
            }
            Press::Back => app.overlay = Overlay::None,
            _ => {}
        },

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
                // A means "do what this tab is for". On sync that is the one
                // row under the cursor — those are actions, with nothing to
                // reconcile, so they ask once and go. On patches it is
                // everything queued.
                let action = match app.page().selected() {
                    Some(row) if matches!(row.kind, Kind::Action { .. }) => {
                        Some(row.title.clone())
                    }
                    _ => None,
                };
                if let Some(title) = action {
                    app.overlay = Overlay::ConfirmAction { title };
                } else if !app.queue().is_empty() {
                    app.overlay = Overlay::ConfirmApply;
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
    fn the_button_printed_a_is_the_one_that_confirms() {
        // SDL names buttons by Xbox position, so its `A` is the bottom one.
        // This handheld is laid out like a Nintendo pad and the button printed
        // A is on the right, which SDL calls `B`. EmulationStation inverts for
        // exactly this reason, and so must we — the press log showed four
        // presses arriving as Back where apply was meant.
        assert_eq!(from_button(Button::B, true), Some(Press::Accept));
        assert_eq!(from_button(Button::A, true), Some(Press::Back));
        // And an Xbox-layout device is left alone.
        assert_eq!(from_button(Button::A, false), Some(Press::Accept));
        assert_eq!(from_button(Button::B, false), Some(Press::Back));
        // The d-pad is unaffected either way.
        assert_eq!(from_button(Button::DPadUp, true), Some(Press::Up));
    }

    #[test]
    fn inversion_is_read_from_emulationstation() {
        let dir = std::env::temp_dir().join("moose-invert");
        let _ = std::fs::remove_dir_all(&dir);
        let paths = Paths::new(&dir);
        std::fs::create_dir_all(paths.es_settings().parent().unwrap()).unwrap();

        assert!(!buttons_inverted(&paths), "no file means no inversion");
        std::fs::write(
            paths.es_settings(),
            "<config>\n\t<bool name=\"InvertButtons\" value=\"true\" />\n</config>\n",
        )
        .unwrap();
        assert!(buttons_inverted(&paths));
        std::fs::write(
            paths.es_settings(),
            "<config>\n\t<bool name=\"InvertButtons\" value=\"false\" />\n</config>\n",
        )
        .unwrap();
        assert!(!buttons_inverted(&paths));
    }

    #[test]
    fn a_on_the_sync_tab_runs_that_row_rather_than_the_patch_queue() {
        // The two tabs mean different things by A. Queuing a patch change and
        // then pressing A on "Push saves up" must not apply the patch — that
        // is a different tab's business and a nasty surprise.
        let mut f = fixture("sync-a");
        press(&mut f, Press::Right);
        assert_eq!(f.0.queue().len(), 1);

        press(&mut f, Press::TabLeft);
        assert_eq!(f.0.tab, Tab::Sync);
        press(&mut f, Press::Accept);
        match &f.0.overlay {
            Overlay::ConfirmAction { title } => assert!(title.contains("Push")),
            other => panic!("expected the action prompt, got {other:?}"),
        }

        // Cancelling leaves both the queue and the device untouched.
        press(&mut f, Press::Back);
        assert_eq!(f.0.overlay, Overlay::None);
        assert_eq!(f.0.queue().len(), 1);
        assert!(!f.2.knulli_conf().exists());
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
