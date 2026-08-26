//! moose-patch — the addon.
//!
//! Two tabs, a list of dials, and one deliberate moment where everything you
//! turned actually happens. See `docs/knulli-addon.md` for what it is for and
//! `model` for why nothing applies as you touch it.

use anyhow::{Context, Result};
use moose_patch::model::{App, Kind, Overlay, Tab};
use moose_patch::{rows, ui};
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
/// one — which is what this device calls A too. `input::index_of` is the
/// translation the rest of the project uses; this is the only place that
/// needs the letters rather than the indices.
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
    // judgement about it was wrong.
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

    let window = open_window(&video)
        .map_err(anyhow::Error::msg)
        .context("opening a window")?;
    let _context = window
        .gl_create_context()
        .map_err(anyhow::Error::msg)
        .context("creating an OpenGL context")?;
    window.gl_set_context_to_current().map_err(anyhow::Error::msg)?;

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
        sync: rows::sync(),
        patches: rows::patches(),
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
            act(&mut app, &mut view, press);
        }

        gfx.resize(ui::PANEL.0 as f32, ui::PANEL.1 as f32);
        view.draw(&gfx, &mut painter, &app);
        window.gl_swap_window();
    }
    Ok(())
}

/// One press. Split out so the whole of the app's behaviour can be exercised
/// without a window — see the tests at the foot of this file.
fn act(app: &mut App, view: &mut ui::Ui, press: Press) {
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
                // Where the scripts will run. Settling is what they leave
                // behind: the device now *is* what the dials say.
                let total = app.queue().len();
                app.sync.settle_all();
                app.patches.settle_all();
                app.overlay = Overlay::Applying { done: total, total };
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
                    // Push, pull, take offline. Nothing to reconcile, so these
                    // run on their own rather than joining the queue.
                    // Not wired yet.
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

    fn app() -> (App, ui::Ui) {
        (
            App {
                tab: Tab::Patches,
                sync: rows::sync(),
                patches: rows::patches(),
                overlay: Overlay::None,
                should_quit: false,
            },
            ui::Ui::default(),
        )
    }

    #[test]
    fn b_leaves_straight_away_when_nothing_is_queued() {
        let (mut a, mut v) = app();
        act(&mut a, &mut v, Press::Back);
        assert!(a.should_quit);
    }

    #[test]
    fn b_asks_first_when_something_is_queued() {
        // The whole point of queuing: work in hand must not vanish because a
        // thumb found the wrong button.
        let (mut a, mut v) = app();
        act(&mut a, &mut v, Press::Right);
        act(&mut a, &mut v, Press::Back);
        assert_eq!(a.overlay, Overlay::ConfirmDiscard);
        assert!(!a.should_quit);
    }

    #[test]
    fn a_never_applies_without_confirming() {
        let (mut a, mut v) = app();
        act(&mut a, &mut v, Press::Right);
        let queued = a.queue().len();
        act(&mut a, &mut v, Press::Accept);
        assert_eq!(a.overlay, Overlay::ConfirmApply);
        assert_eq!(a.queue().len(), queued, "nothing may be applied yet");
    }

    #[test]
    fn confirming_empties_the_queue_and_cancelling_does_not() {
        let (mut a, mut v) = app();
        act(&mut a, &mut v, Press::Right);
        act(&mut a, &mut v, Press::Accept);
        act(&mut a, &mut v, Press::Back);
        assert_eq!(a.queue().len(), 1, "cancelling keeps the queue");
        act(&mut a, &mut v, Press::Accept);
        act(&mut a, &mut v, Press::Accept);
        assert!(a.queue().is_empty(), "confirming applies it");
        assert_eq!(a.overlay, Overlay::None);
    }

    #[test]
    fn discarding_puts_every_dial_back() {
        let (mut a, mut v) = app();
        act(&mut a, &mut v, Press::Right);
        act(&mut a, &mut v, Press::Down);
        act(&mut a, &mut v, Press::Right);
        assert_eq!(a.queue().len(), 2);
        act(&mut a, &mut v, Press::Back);
        act(&mut a, &mut v, Press::Accept);
        assert!(a.queue().is_empty());
        assert!(a.should_quit);
    }

    #[test]
    fn the_queue_survives_changing_tabs() {
        // Turning a dial on one tab and wandering to the other must not
        // quietly drop it — the counter says "not applied", and it has to
        // mean it.
        let (mut a, mut v) = app();
        act(&mut a, &mut v, Press::Right);
        act(&mut a, &mut v, Press::TabLeft);
        assert_eq!(a.tab, Tab::Sync);
        assert_eq!(a.queue().len(), 1);
    }

    #[test]
    fn x_opens_the_detail_and_b_closes_it() {
        let (mut a, mut v) = app();
        act(&mut a, &mut v, Press::Detail);
        assert_eq!(a.overlay, Overlay::Detail);
        act(&mut a, &mut v, Press::Back);
        assert_eq!(a.overlay, Overlay::None);
        assert!(!a.should_quit, "closing a panel is not leaving the app");
    }

    #[test]
    fn dials_do_nothing_while_a_panel_is_up() {
        let (mut a, mut v) = app();
        act(&mut a, &mut v, Press::Detail);
        act(&mut a, &mut v, Press::Right);
        assert!(a.queue().is_empty(), "the list is not live behind a panel");
    }
}
