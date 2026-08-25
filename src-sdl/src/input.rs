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

/// Where KNULLI describes the pads it knows about.
///
/// The same file EmulationStation reads. Reading it rather than writing the one
/// pad down here means this works on the next device too — a handheld's
/// controls are always in here, and they are always the ones the OS is already
/// using.
const ES_INPUT: &str = "/usr/share/emulationstation/es_input.cfg";

/// Which SDL name each of ES's input names goes by.
///
/// ES names the buttons after what is printed on them and SDL names them after
/// where they sit on an Xbox pad. `hotkey` is the odd one: on a handheld it is
/// whatever the maker chose as a modifier, and SDL calls that slot `guide`.
const NAMES: &[(&str, &str)] = &[
    ("a", "a"),
    ("b", "b"),
    ("x", "x"),
    ("y", "y"),
    ("pageup", "leftshoulder"),
    ("pagedown", "rightshoulder"),
    ("l2", "lefttrigger"),
    ("r2", "righttrigger"),
    ("l3", "leftstick"),
    ("r3", "rightstick"),
    ("select", "back"),
    ("start", "start"),
    ("hotkey", "guide"),
    ("up", "dpup"),
    ("down", "dpdown"),
    ("left", "dpleft"),
    ("right", "dpright"),
];

/// The axes, which ES names by the direction rather than the axis.
const AXES: &[(&str, &str)] = &[
    ("joystick1left", "leftx"),
    ("joystick1up", "lefty"),
    ("joystick2left", "rightx"),
    ("joystick2up", "righty"),
];

/// Every pad in `es_input.cfg`, as SDL mapping strings.
fn es_input_mappings() -> Vec<(String, String)> {
    let Ok(text) = std::fs::read_to_string(ES_INPUT) else {
        return Vec::new();
    };
    mappings_from(&text)
}

/// The parsing, apart from the file, so it can be tested.
pub fn mappings_from(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for block in text.split("<inputConfig").skip(1) {
        let block = block.split("</inputConfig>").next().unwrap_or(block);
        if attr(block, "type").as_deref() != Some("joystick") {
            continue;
        }
        let (Some(guid), Some(name)) = (attr(block, "deviceGUID"), attr(block, "deviceName"))
        else {
            continue;
        };
        // A name with a comma in it would split the mapping into fields that
        // are not fields.
        let name = name.replace(',', " ");
        let mut parts = vec![guid.clone(), name];
        for line in block.split("<input").skip(1) {
            let (Some(what), Some(kind), Some(id)) =
                (attr(line, "name"), attr(line, "type"), attr(line, "id"))
            else {
                continue;
            };
            match kind.as_str() {
                "button" => {
                    if let Some((_, sdl)) = NAMES.iter().find(|(es, _)| *es == what) {
                        parts.push(format!("{sdl}:b{id}"));
                    }
                }
                "axis" => {
                    if let Some((_, sdl)) = AXES.iter().find(|(es, _)| *es == what) {
                        parts.push(format!("{sdl}:a{id}"));
                    }
                }
                // A d-pad reported as a hat rather than as four buttons. Both
                // shapes are in this file across devices.
                "hat" => {
                    if let Some((_, sdl)) = NAMES.iter().find(|(es, _)| *es == what) {
                        let value = attr(line, "value").unwrap_or_else(|| "1".to_owned());
                        parts.push(format!("{sdl}:h{id}.{value}"));
                    }
                }
                _ => {}
            }
        }
        // Two fields and nothing else is a pad with no buttons described, and
        // SDL would take it and then report every button unpressed.
        if parts.len() < 6 {
            continue;
        }
        parts.push("platform:Linux".to_owned());
        out.push((guid, parts.join(",")));
    }
    out
}

/// One `name="value"` out of a tag.
fn attr(block: &str, name: &str) -> Option<String> {
    let key = format!("{name}=\"");
    let start = block.find(&key)? + key.len();
    let end = block[start..].find('"')? + start;
    Some(block[start..end].to_owned())
}

impl Pads {
    pub fn open_first(subsystem: &sdl2::GameControllerSubsystem) -> Self {
        let count = subsystem.num_joysticks().unwrap_or(0);
        // Teach SDL about pads it does not already know.
        //
        // The Flip's built-in controls are a plain joystick as far as SDL is
        // concerned: no entry in its mapping database, so `is_game_controller`
        // says no, `open_first` finds nothing, and every button on the device
        // does nothing at all. That is not a fault in the pad — the OS knows
        // exactly what it is, in the same file its own front end reads.
        for (guid, mapping) in es_input_mappings() {
            if subsystem.add_mapping(&mapping).is_err() {
                eprintln!("could not add the mapping for {guid}");
            }
        }
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

#[cfg(test)]
mod mapping {
    use super::*;

    /// The Flip's own entry, out of the device's own file.
    const FRAGMENT: &str = include_str!("../tests/data/es_input_fragment.cfg");

    /// The pad the device knows about becomes a pad SDL knows about.
    ///
    /// Without this every button on the handheld does nothing: SDL sees a plain
    /// joystick with no mapping, `is_game_controller` says no, and the app
    /// opens no pad at all. The names on the two sides do not match either —
    /// ES calls the shoulders `pageup`/`pagedown` and the modifier `hotkey`,
    /// SDL calls them `leftshoulder`/`rightshoulder` and `guide`.
    #[test]
    fn the_devices_own_pad_becomes_an_sdl_mapping() {
        let all = mappings_from(FRAGMENT);
        let flip = all
            .iter()
            .find(|(guid, _)| guid == "190000004b4800000111000000010000")
            .map(|(_, m)| m.clone())
            .expect("the Flip's pad was not read");

        for want in [
            "a:b0", "b:b1", "x:b2", "y:b3",
            "leftshoulder:b4", "rightshoulder:b5",
            "lefttrigger:b6", "righttrigger:b7",
            "back:b8", "start:b9", "guide:b10",
            "leftstick:b11", "rightstick:b12",
            "dpup:b13", "dpdown:b14", "dpleft:b15", "dpright:b16",
            "leftx:a0", "lefty:a1", "rightx:a2", "righty:a3",
            "platform:Linux",
        ] {
            assert!(flip.contains(want), "{want} missing from:\n{flip}");
        }
        assert!(flip.starts_with("190000004b4800000111000000010000,Miyoo Flip Controller,"));
    }

    /// A keyboard is not a pad, and a pad with one button described is not a
    /// pad SDL should be told about — it would take the mapping and then report
    /// every other button unpressed forever.
    #[test]
    fn only_real_pads_are_offered() {
        let all = mappings_from(FRAGMENT);
        assert_eq!(all.len(), 1, "{all:#?}");
    }

    /// A comma in a device name would split the mapping into fields that are
    /// not fields.
    #[test]
    fn a_comma_in_a_name_cannot_break_the_format() {
        let text = FRAGMENT.replace(
            r#"deviceName="Miyoo Flip Controller""#,
            r#"deviceName="Odd, Pad""#,
        );
        let (_, mapping) = mappings_from(&text).into_iter().next().unwrap();
        let fields: Vec<&str> = mapping.split(',').collect();
        assert_eq!(fields[1], "Odd  Pad", "the comma survived into the name");
        assert!(fields[2].contains(':'), "the fields shifted: {:?}", &fields[..4]);
    }

    /// A missing file is no mappings rather than a panic — this runs on a Mac
    /// too, where there is no such file and the pad needs no help.
    #[test]
    fn no_file_is_no_mappings() {
        assert!(mappings_from("").is_empty());
        assert!(mappings_from("<inputList></inputList>").is_empty());
    }
}
