// Keyboard and pad, resolved through the tables in `romm_desktop::binds`.
//
// SDL and the W3C standard mapping disagree about which integer is which
// button, so everything here translates into the indices
// `binds::PAD_BUTTONS` names before asking anything. That vocabulary is the
// contract: a rebind made in the desktop app means the same thing here.

use romm_desktop::{binds, padpoll};
use sdl2::controller::{Axis, Button, GameController};
use sdl2::keyboard::Keycode;
use std::collections::BTreeSet;

/// SDL's button, as the index the bindings are written against.
///
/// The order is the W3C standard mapping, which is what
/// `binds::PAD_BUTTONS` documents: 0 is the bottom face button, which is A on
/// Xbox, Cross on PlayStation and B on a Nintendo pad. SDL names its buttons
/// after the Xbox layout, so `Button::A` is the bottom one and the translation
/// is by position, not by letter.
pub fn index_of(button: Button) -> Option<u8> {
    Some(match button {
        Button::A => 0,
        Button::B => 1,
        Button::X => 2,
        Button::Y => 3,
        Button::LeftShoulder => 4,
        Button::RightShoulder => 5,
        // 6 and 7 are the triggers, which SDL reports as axes rather than
        // buttons. See `Pads::pressed`.
        Button::Back => 8,
        Button::Start => 9,
        Button::LeftStick => 10,
        Button::RightStick => 11,
        Button::DPadUp => 12,
        Button::DPadDown => 13,
        Button::DPadLeft => 14,
        Button::DPadRight => 15,
        _ => return None,
    })
}

/// A key, in the names the bindings use.
///
/// The webview's names, because that is what is already in people's
/// `config.toml` — `ArrowLeft` rather than SDL's `Left`. Anything with no name
/// there is passed through as the character it produces, which is what a
/// single-letter binding is.
fn name_of(key: Keycode) -> String {
    match key {
        Keycode::Left => "ArrowLeft".into(),
        Keycode::Right => "ArrowRight".into(),
        Keycode::Up => "ArrowUp".into(),
        Keycode::Down => "ArrowDown".into(),
        Keycode::Return | Keycode::KpEnter => "Enter".into(),
        Keycode::Escape => "Escape".into(),
        Keycode::Backspace => "Backspace".into(),
        Keycode::Home => "Home".into(),
        Keycode::End => "End".into(),
        Keycode::PageUp => "PageUp".into(),
        Keycode::PageDown => "PageDown".into(),
        Keycode::Space => " ".into(),
        other => other.name().to_lowercase(),
    }
}

pub fn action_for_key(bindings: &binds::Bindings, key: Keycode) -> Option<&'static str> {
    bindings.action_for(&name_of(key))
}

/// The pad that drives the interface: the first one connected, and only that
/// one.
///
/// With four controllers plugged in for a four-player game, every one of them
/// moving the cursor makes the menu unusable — three other people fidgeting
/// while player one tries to pick something. In the emulator all four are
/// players; out here, player one is in charge.
pub struct Pads {
    held: Option<GameController>,
}

impl Pads {
    pub fn open_first(subsystem: &sdl2::GameControllerSubsystem) -> Self {
        let count = subsystem.num_joysticks().unwrap_or(0);
        for index in 0..count {
            if !subsystem.is_game_controller(index) {
                continue;
            }
            if let Ok(pad) = subsystem.open(index) {
                println!("controller: {}", pad.name());
                return Pads { held: Some(pad) };
            }
        }
        Pads { held: None }
    }

    /// Which actions the pad is asking for this frame.
    ///
    /// Read as state rather than as events, because that is what
    /// `padpoll::pressed_actions` is written against — how far a stick is
    /// pushed is a number, not an edge, and a held direction has to keep
    /// saying so every frame for the repeat to work.
    pub fn pressed(
        &mut self,
        map: &std::collections::BTreeMap<u8, Option<String>>,
    ) -> BTreeSet<String> {
        let Some(pad) = self.held.as_ref() else {
            return BTreeSet::new();
        };
        let mut buttons = vec![false; 16];
        for button in [
            Button::A,
            Button::B,
            Button::X,
            Button::Y,
            Button::LeftShoulder,
            Button::RightShoulder,
            Button::Back,
            Button::Start,
            Button::LeftStick,
            Button::RightStick,
            Button::DPadUp,
            Button::DPadDown,
            Button::DPadLeft,
            Button::DPadRight,
        ] {
            if let Some(i) = index_of(button) {
                buttons[i as usize] = pad.button(button);
            }
        }
        // The triggers are analogue on any pad worth the name, so SDL reports
        // them as axes. Past the deadzone counts as pressed, which is what
        // gives a digital trigger somewhere to land — `padpoll::trigger_scroll`
        // is what reads the pull itself, later.
        buttons[6] = axis(pad, Axis::TriggerLeft) > padpoll::TRIGGER_DEADZONE;
        buttons[7] = axis(pad, Axis::TriggerRight) > padpoll::TRIGGER_DEADZONE;

        let axes = [axis(pad, Axis::LeftX), axis(pad, Axis::LeftY)];
        padpoll::pressed_actions(&buttons, &axes, map)
    }

    /// Whichever button is down, as the index the bindings are written
    /// against — for capturing a binding, where a button has to be caught
    /// before it means anything.
    pub fn any_button(&self) -> Option<u8> {
        let pad = self.held.as_ref()?;
        [
            Button::A,
            Button::B,
            Button::X,
            Button::Y,
            Button::LeftShoulder,
            Button::RightShoulder,
            Button::Back,
            Button::Start,
            Button::LeftStick,
            Button::RightStick,
            Button::DPadUp,
            Button::DPadDown,
            Button::DPadLeft,
            Button::DPadRight,
        ]
        .into_iter()
        .filter(|b| pad.button(*b))
        .find_map(index_of)
    }

    /// The right stick, for scrolling the preview. Signed, and shaped by
    /// `padpoll` so a small push creeps and a full one moves properly.
    #[allow(dead_code)]
    pub fn detail_scroll(&self) -> f64 {
        self.held
            .as_ref()
            .map(|pad| padpoll::stick_scroll(axis(pad, Axis::RightY)))
            .unwrap_or(0.0)
    }
}

/// One axis, as the -1..1 the core works in. SDL reports i16.
fn axis(pad: &GameController, which: Axis) -> f64 {
    pad.axis(which) as f64 / i16::MAX as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The vocabulary is the contract. A binding written by the desktop app
    /// has to mean the same button here, and SDL numbers its own differently.
    #[test]
    fn sdl_buttons_land_on_the_indices_the_bindings_name() {
        assert_eq!(
            index_of(Button::A),
            Some(0),
            "the bottom face button is not Confirm"
        );
        assert_eq!(index_of(Button::B), Some(1));
        assert_eq!(index_of(Button::DPadUp), Some(12));
        assert_eq!(index_of(Button::DPadRight), Some(15));
        // Every index we claim is one the tables actually describe.
        for button in [
            Button::A,
            Button::B,
            Button::X,
            Button::Y,
            Button::Back,
            Button::Start,
        ] {
            let index = index_of(button).expect("mapped");
            assert!(
                binds::PAD_BUTTONS.iter().any(|b| b.index == index),
                "{button:?} maps to {index}, which is not a button the core names"
            );
        }
    }

    /// The names in people's config.toml were written by a browser, and this
    /// front end has to read them unchanged. `ArrowLeft`, not SDL's `Left`.
    #[test]
    fn keys_are_named_the_way_the_webview_named_them() {
        let b = binds::Bindings::default();
        assert_eq!(action_for_key(&b, Keycode::Left), Some("left"));
        assert_eq!(action_for_key(&b, Keycode::Return), Some("activate"));
        assert_eq!(action_for_key(&b, Keycode::Escape), Some("back"));
        assert_eq!(action_for_key(&b, Keycode::Home), Some("first"));
        assert_eq!(action_for_key(&b, Keycode::PageDown), Some("pageDown"));
    }

    /// A single letter is bound as the character it produces, whatever case
    /// the key is reported in.
    #[test]
    fn a_letter_binding_works_from_either_case() {
        let b = binds::Bindings::default();
        assert_eq!(action_for_key(&b, Keycode::S), Some("sortMenu"));
        assert_eq!(action_for_key(&b, Keycode::F), Some("filterMenu"));
        assert_eq!(action_for_key(&b, Keycode::R), Some("random"));
    }

    /// A rebind made in the desktop app is a rebind here — same file, same
    /// resolution, no second table.
    #[test]
    fn a_rebind_from_the_desktop_app_is_read_here() {
        let mut b = binds::Bindings::default();
        b.set_key("random", Some("z"));
        assert_eq!(action_for_key(&b, Keycode::Z), Some("random"));
        assert_eq!(
            action_for_key(&b, Keycode::R),
            None,
            "the old key still works"
        );
    }

    /// A key nothing is bound to resolves to nothing, rather than to whatever
    /// happens to be first.
    #[test]
    fn an_unbound_key_does_nothing() {
        let b = binds::Bindings::default();
        assert_eq!(action_for_key(&b, Keycode::F13), None);
    }
}
