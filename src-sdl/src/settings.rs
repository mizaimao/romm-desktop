// Settings, as data rather than as a screen.
//
// The desktop has 38 of these across seven panes, and a survey of them found
// that all but a handful are one of four shapes: on or off, one of a fixed set,
// a number in a range, or a line of text. So the screen is one list widget and
// this table, and adding a setting is a row here rather than markup, a wiring
// function and a backend allowlist entry.
//
// What is deliberately *not* here: the four the desktop has that mean nothing on
// a handheld — the window arrangement, the title bar, fit-to-window, and where
// RetroArch is installed, which on KNULLI is `/usr` and decided by the platform
// scheme. A setting that cannot do anything is worse than a missing one,
// because you try it.
//
// Writing goes through `romm_desktop::config`, which is the same three functions
// the desktop app's Tauri commands call. There is no second writer and no second
// idea of what a field is called.

use anyhow::Context;
use romm_desktop::config::{self, Config};

/// The file this front end reads and writes.
///
/// Not `config.toml`. The desktop app owns that name, and on a development Mac
/// both run out of the same directory — so a value stepped in the preview was
/// editing the real desktop settings, silently, one press at a time. On the
/// device there is only ever one app and the name is simply explicit about
/// which one it belongs to.
///
/// Same format and same `Config` type: this is a second *file*, not a second
/// idea of what a setting is.
pub const FILE: &str = "romm-sdl.toml";

/// Read the front end's config, falling back to the desktop's if it has none
/// yet.
///
/// The fallback is read-only and happens once: a first run on a machine that
/// already has a server and an account configured should not start from
/// nothing. Everything written afterwards goes to [`FILE`], so the two never
/// drift into each other.
pub fn load() -> (Config, &'static str) {
    if Config::exists(FILE) {
        return (
            Config::load_from(std::path::Path::new(FILE)).unwrap_or_default(),
            FILE,
        );
    }
    if Config::exists("config.toml") {
        return (
            Config::load_from(std::path::Path::new("config.toml")).unwrap_or_default(),
            "config.toml",
        );
    }
    (Config::default(), FILE)
}

/// What kind of control a setting is.
#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    /// On or off.
    Toggle(bool),
    /// One of a fixed set. `at` indexes `options`, which is `(value, label)`.
    ///
    /// Owned rather than borrowed because the emulator rows are built from the
    /// core map at run time — which cores a console has is not something that
    /// can be written down in the source.
    Choice {
        at: usize,
        options: Vec<(String, String)>,
    },
    /// A number in a range, stepped with left and right.
    Number {
        value: i64,
        min: i64,
        max: i64,
        step: i64,
        unit: &'static str,
    },
    /// A line of text, filled in with the on-screen keyboard.
    Text { value: String, secret: bool },
    /// Shown, not changed. The help says where to change it.
    ReadOnly(String),
    /// Does something rather than holding a value.
    Action,
}

/// One row on a settings screen.
#[derive(Debug, Clone)]
pub struct Entry {
    /// The `[table] key` this writes to, as `table.key`. Empty for actions and
    /// for anything the config does not own.
    pub field: &'static str,
    pub label: &'static str,
    /// One sentence, shown beside the list. Says what it does, or where to
    /// change it when it cannot be changed here.
    pub help: &'static str,
    pub kind: Kind,
}

impl Entry {
    /// What the right-hand column shows.
    pub fn value(&self) -> String {
        match &self.kind {
            Kind::Toggle(on) => if *on { "On" } else { "Off" }.to_owned(),
            Kind::Choice { at, options } => options
                .get(*at)
                .map(|(_, label)| (*label).to_owned())
                .unwrap_or_default(),
            Kind::Number { value, unit, .. } => format!("{value}{unit}"),
            Kind::Text { value, secret } => {
                if value.is_empty() {
                    "not set".to_owned()
                } else if *secret {
                    "\u{2022}".repeat(value.chars().count().min(12))
                } else {
                    value.clone()
                }
            }
            Kind::ReadOnly(v) => v.clone(),
            Kind::Action => "\u{203a}".to_owned(),
        }
    }

    /// Whether left and right change this in place.
    pub fn steps(&self) -> bool {
        matches!(
            self.kind,
            Kind::Toggle(_) | Kind::Choice { .. } | Kind::Number { .. }
        )
    }

    /// Step the value. `dir` is -1 or 1. Returns what to write, if anything.
    ///
    /// Toggles ignore the direction — there are two states and both directions
    /// reach the other one, which is what every settings list does and what
    /// stops "left" being a dead press on half the rows.
    pub fn step(&mut self, dir: i64) -> Option<Written> {
        match &mut self.kind {
            Kind::Toggle(on) => {
                *on = !*on;
                Some(Written::Bool(*on))
            }
            Kind::Choice { at, options } => {
                let n = options.len();
                if n == 0 {
                    return None;
                }
                *at = (*at as i64 + dir).rem_euclid(n as i64) as usize;
                Some(Written::Text(options[*at].0.clone()))
            }
            Kind::Number {
                value,
                min,
                max,
                step,
                ..
            } => {
                *value = (*value + dir * *step).clamp(*min, *max);
                Some(Written::Number(*value))
            }
            _ => None,
        }
    }
}

/// A value on its way to `config.toml`.
#[derive(Debug, Clone, PartialEq)]
pub enum Written {
    Bool(bool),
    Number(i64),
    Text(String),
}

/// A group of settings, which is one screen.
pub struct Pane {
    pub id: &'static str,
    pub label: &'static str,
    pub entries: Vec<Entry>,
}

fn show_options() -> Vec<(String, String)> {
    vec![
        ("all".into(), "Everything".into()),
        ("downloaded".into(), "On this device".into()),
    ]
}

/// The Device pane, built from whatever the platform scheme actually offers.
///
/// A row per thing the hardware has, and no row for a thing it does not — on a
/// Mac there is no backlight and no battery, so the pane is short rather than
/// full of controls that do nothing. Which files these read is
/// `romm_desktop::platform`, measured on the Flip.
fn device_entries() -> Vec<Entry> {
    let p = romm_desktop::platform::current();
    let mut out = Vec::new();

    if p.wifi().is_some() {
        out.push(Entry {
            field: "",
            label: "Wi-Fi",
            help: "Join a network. Scanning uses the device's own knulli-wifi.",
            kind: Kind::Action,
        });
    }
    if let Some(b) = p.brightness() {
        // Read the real level, so the row opens on where the screen actually
        // is rather than snapping it to a default the moment you press right.
        let now = std::fs::read_to_string(&b.path)
            .ok()
            .and_then(|s| s.trim().parse::<i64>().ok())
            .unwrap_or((b.max / 2) as i64);
        out.push(Entry {
            field: "device.brightness",
            label: "Brightness",
            help: "Screen backlight. Left and right change it straight away.",
            kind: Kind::Number {
                value: now,
                min: 0,
                max: b.max as i64,
                step: (b.max / 16).max(1) as i64,
                unit: "",
            },
        });
    }
    if let Some(b) = p.battery() {
        let charge = std::fs::read_to_string(&b.capacity)
            .ok()
            .map(|s| format!("{}%", s.trim()))
            .unwrap_or_else(|| "unknown".into());
        out.push(Entry {
            field: "",
            label: "Battery",
            help: "The gauge on this unit reports itself as uncalibrated, so treat the percentage as rough.",
            kind: Kind::ReadOnly(charge),
        });
    }
    if out.is_empty() {
        out.push(Entry {
            field: "",
            label: "No device controls",
            help: "This build has no backlight, battery or radio to offer — it is running on a desktop.",
            kind: Kind::ReadOnly(String::new()),
        });
    }
    out
}

/// One row per console, offering the cores that console actually has.
///
/// Ours, not KNULLI's. This front end replaces the KNULLI front end and launches
/// RetroArch itself through `launch::plan`, so configgen never sees the choice
/// and `knulli.conf` is not consulted — which is just as well, because
/// configgen takes a core name verbatim with no validation and a name that does
/// not exist becomes a failed launch rather than an error.
///
/// The defaults are the picks settled on the device: `pcsx_rearmed` for PSX
/// rather than SwanStation, `flycastvl` rather than full Flycast, and Neo Geo
/// fixed to `geolith` because the ROMs here are geolith's.
fn emulator_entries(
    consoles: &[(String, String)],
    map: &romm_desktop::coremap::CoreMap,
    overrides: &std::collections::BTreeMap<String, String>,
) -> Vec<Entry> {
    consoles
        .iter()
        .filter_map(|(slug, name)| {
            let mut cores: Vec<&str> = map.alternatives(slug);
            if let Some(d) = map.default_core(slug)
                && !cores.contains(&d)
            {
                cores.insert(0, d);
            }
            if cores.len() < 2 {
                // One core is not a choice. A row that cannot change is noise
                // on a screen you came to to change something.
                return None;
            }
            // What you chose, then what this device should run, then the
            // shipped default. The middle one is the point: the core map is
            // ES-DE's and assumes a desktop.
            let device_pick = romm_desktop::platform::current()
                .default_cores()
                .iter()
                .find(|(p, _)| *p == slug)
                .map(|(_, c)| *c);
            let chosen = overrides
                .get(slug)
                .map(String::as_str)
                .or(device_pick)
                .or_else(|| map.default_core(slug))
                .unwrap_or(cores[0]);
            // A device pick the map does not list is still offered — it is the
            // one that runs here.
            if let Some(pick) = device_pick
                && !cores.contains(&pick)
            {
                cores.insert(0, pick);
            }
            let at = cores.iter().position(|c| *c == chosen).unwrap_or(0);
            let options: Vec<(String, String)> = cores
                .iter()
                .map(|c| {
                    let label = map.label_for(c).unwrap_or(c);
                    ((*c).to_owned(), label.to_owned())
                })
                .collect();
            Some(Entry {
                // Leaked because an entry names its field for the life of the
                // program and there are a few dozen of them, once, at startup.
                field: Box::leak(format!("cores_overrides.{slug}").into_boxed_str()),
                label: Box::leak(name.clone().into_boxed_str()),
                help: "Which core runs this console. The default is what the device was measured to run well.",
                kind: Kind::Choice { at, options },
            })
        })
        .collect()
}

/// One row per action, showing the pad button it is on.
///
/// Pad only. A handheld has no keyboard to bind, and a keyboard column here
/// would be a column of the same dash 29 times — the key bindings still exist
/// and are still edited in the desktop app, which is where somebody with a
/// keyboard is sitting.
///
/// Not every action earns a row: the ones with no pad button and no sensible
/// one to give them are the desktop's, and offering to bind them here says the
/// device can do something it cannot.
fn binding_entries(binds: &romm_desktop::binds::Bindings) -> Vec<Entry> {
    romm_desktop::binds::ACTIONS
        .iter()
        .filter(|a| !matches!(a.id, "search" | "help" | "sortMenu" | "filterMenu"))
        .map(|a| Entry {
            field: Box::leak(format!("bindings_pad.{}", a.id).into_boxed_str()),
            label: a.label,
            help: "Press left or right to move this to another button.",
            kind: Kind::Choice {
                at: pad_index(binds.pad_for(a.id)),
                options: pad_options(),
            },
        })
        .collect()
}

/// The buttons a binding can be on, plus "not bound".
///
/// A fixed list rather than a capture mode: capture means "press the button you
/// want", which on a device where every button already does something means
/// pressing the one that leaves the screen. Stepping through the names is
/// slower and cannot trap you.
fn pad_options() -> Vec<(String, String)> {
    let mut out = vec![(String::new(), "not bound".to_owned())];
    for b in romm_desktop::binds::PAD_BUTTONS {
        out.push((
            b.index.to_string(),
            romm_desktop::binds::pad_label(Some(b.index)),
        ));
    }
    out
}

fn pad_index(button: Option<u8>) -> usize {
    match button {
        None => 0,
        Some(b) => romm_desktop::binds::PAD_BUTTONS
            .iter()
            .position(|p| p.index == b)
            .map(|i| i + 1)
            .unwrap_or(0),
    }
}

/// Build every pane from the config as it stands.
///
/// Read once when the tab is opened rather than held live: a settings screen
/// that re-reads the file while you are on it can show a value you did not set,
/// and nothing else writes that file while this app is running.
pub fn panes(
    cfg: &Config,
    consoles: &[(String, String)],
    map: &romm_desktop::coremap::CoreMap,
) -> Vec<Pane> {
    let emulators = emulator_entries(consoles, map, &cfg.cores.overrides);
    let bindings = binding_entries(&cfg.bindings);
    vec![
        Pane {
            id: "device",
            label: "Device",
            // The pane the desktop does not have. Everything here is the
            // handheld's own hardware, and every path behind it was measured on
            // the Flip — see `romm_desktop::platform::knulli`. Built from what
            // the scheme actually offers, so a desktop build gets a short pane
            // rather than controls that do nothing.
            entries: device_entries(),
        },
        Pane {
            id: "library",
            label: "Library",
            entries: vec![
                Entry {
                    field: "library.local_root",
                    label: "Games folder",
                    help: "Set by the device image. Change it in romm-sdl.toml on the card.",
                    kind: Kind::ReadOnly(cfg.library.local_root.clone()),
                },
                Entry {
                    field: "library.romm_collections",
                    label: "RomM collections",
                    help: "Show the collections RomM generates by company, genre and franchise alongside your own.",
                    kind: Kind::Toggle(cfg.library.romm_collections),
                },
                Entry {
                    field: "media.list_art",
                    label: "Game list shows",
                    help: "Which games appear in a console's list.",
                    kind: Kind::Choice {
                        at: 0,
                        options: show_options(),
                    },
                },
            ],
        },
        Pane {
            id: "accounts",
            label: "Accounts",
            entries: vec![
                Entry {
                    field: "server.url",
                    label: "RomM server",
                    help: "Where the library is synced from.",
                    kind: Kind::Text {
                        value: cfg.server.url.clone(),
                        secret: false,
                    },
                },
                Entry {
                    field: "achievements.enabled",
                    label: "RetroAchievements",
                    help: "Track achievements while you play.",
                    kind: Kind::Toggle(cfg.achievements.enabled),
                },
                Entry {
                    field: "achievements.username",
                    label: "Achievements login",
                    help: "Your RetroAchievements username.",
                    kind: Kind::Text {
                        value: cfg.achievements.username.clone().unwrap_or_default(),
                        secret: false,
                    },
                },
                Entry {
                    field: "achievements.token",
                    label: "Achievements token",
                    help: "From your RetroAchievements settings page. Not your password.",
                    kind: Kind::Text {
                        value: cfg.achievements.token.clone().unwrap_or_default(),
                        secret: true,
                    },
                },
            ],
        },
        Pane {
            id: "emulators",
            label: "Emulators",
            entries: emulators,
        },
        Pane {
            id: "controls",
            label: "Controls",
            entries: bindings,
        },
        Pane {
            id: "about",
            label: "About",
            entries: vec![
                Entry {
                    field: "",
                    label: "Version",
                    help: "This build.",
                    kind: Kind::ReadOnly(env!("CARGO_PKG_VERSION").to_owned()),
                },
                Entry {
                    field: "",
                    label: "Device",
                    help: "Which platform scheme this build selected.",
                    kind: Kind::ReadOnly(romm_desktop::platform::current().scheme().to_owned()),
                },
            ],
        },
    ]
}

/// Write one setting to [`FILE`].
///
/// The same three functions the desktop's Tauri command calls, so a value set
/// here and a value set there are written the same way into the same format —
/// into this front end's own file. The field is `table.key`; anything without a
/// dot is not ours to write.
pub fn write(field: &str, value: &Written) -> anyhow::Result<()> {
    let Some((table, key)) = field.split_once('.') else {
        anyhow::bail!("{field} is not a config field");
    };
    // `device.*` is the hardware, not the config file. Backlight is the one
    // setting where the file would be the wrong place entirely: it has to take
    // effect as you hold the button, and the device already remembers it.
    if table == "device" {
        return apply_device(key, value);
    }
    if table == "bindings_pad" {
        return set_pad_binding(key, value);
    }
    let table = toml_table(table);
    match value {
        Written::Bool(v) => config::set_table_bool(FILE, table, key, *v),
        Written::Number(v) => config::set_table_number(FILE, table, key, *v),
        Written::Text(v) => config::set_table_entry(FILE, table, key, v),
    }
}

/// Move an action to another pad button, and save it.
///
/// Through `Bindings::set_pad` rather than by writing the key directly: that
/// method also clears whatever else was on the button, and two actions on one
/// button is a pad where one of them silently stops working.
fn set_pad_binding(action: &str, value: &Written) -> anyhow::Result<()> {
    let Written::Text(v) = value else {
        anyhow::bail!("a binding is a button number or nothing");
    };
    let button = if v.is_empty() {
        None
    } else {
        Some(v.parse::<u8>()?)
    };
    let mut cfg = Config::load_from(std::path::Path::new(FILE)).unwrap_or_default();
    cfg.bindings.set_pad(action, button);
    let entries: Vec<(String, Option<String>)> = cfg
        .bindings
        .pad
        .iter()
        .map(|(k, v)| (k.clone(), Some(v.clone())))
        .collect();
    config::set_table_entries(FILE, "bindings.pad", &entries).context("saving the pad bindings")
}

/// The TOML table a field's prefix means.
///
/// `[cores.overrides]` is a sub-table and `table.key` cannot spell a second
/// dot, so the field says `cores_overrides` and this puts it back. Get it wrong
/// and every core choice lands in a `[cores_overrides]` table nothing reads —
/// which looks exactly like it worked.
fn toml_table(prefix: &str) -> &str {
    match prefix {
        "cores_overrides" => "cores.overrides",
        other => other,
    }
}

/// Apply a hardware setting, now.
///
/// Through the vendor wrapper where there is one — `knulli-brightness` also
/// handles whatever else the board couples to the backlight — and by writing
/// the sysfs node when there is not.
fn apply_device(key: &str, value: &Written) -> anyhow::Result<()> {
    let Written::Number(n) = value else {
        anyhow::bail!("device.{key} takes a number");
    };
    match key {
        "brightness" => {
            let Some(b) = romm_desktop::platform::current().brightness() else {
                anyhow::bail!("this device has no backlight");
            };
            if let Some(helper) = b.helper
                && std::process::Command::new(helper)
                    .arg(n.to_string())
                    .status()
                    .is_ok_and(|s| s.success())
            {
                return Ok(());
            }
            std::fs::write(&b.path, n.to_string())
                .with_context(|| format!("writing {}", b.path.display()))
        }
        other => anyhow::bail!("nothing applies device.{other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A couple of real consoles and the shipped core map, so the emulator
    /// rows in these tests are the ones the app builds.
    fn built() -> Vec<Pane> {
        let cfg = Config::default();
        let consoles = [
            ("megadrive".to_owned(), "Sega Mega Drive".to_owned()),
            ("snes".to_owned(), "Super Nintendo".to_owned()),
        ];
        panes(&cfg, &consoles, &romm_desktop::coremap::CoreMap::embedded())
    }

    fn entry(kind: Kind) -> Entry {
        Entry {
            field: "a.b",
            label: "L",
            help: "H",
            kind,
        }
    }

    /// A toggle flips whichever way you press, because a settings list where
    /// half the rows ignore "left" is a list you stop trusting.
    #[test]
    fn a_toggle_flips_in_either_direction() {
        let mut e = entry(Kind::Toggle(false));
        assert_eq!(e.step(1), Some(Written::Bool(true)));
        assert_eq!(e.value(), "On");
        assert_eq!(e.step(-1), Some(Written::Bool(false)));
        assert_eq!(e.value(), "Off");
    }

    /// A choice wraps, so the last option is one press left of the first.
    #[test]
    fn a_choice_wraps_both_ways() {
        let mut e = entry(Kind::Choice {
            at: 0,
            options: show_options(),
        });
        assert_eq!(e.value(), "Everything");
        assert_eq!(e.step(-1), Some(Written::Text("downloaded".into())));
        assert_eq!(e.value(), "On this device");
        e.step(1);
        assert_eq!(e.value(), "Everything");
    }

    /// A number stops at its ends rather than wrapping — brightness rolling
    /// from full to off in one press is a dark screen and a hunt for the
    /// button.
    #[test]
    fn a_number_clamps_and_does_not_wrap() {
        let mut e = entry(Kind::Number {
            value: 250,
            min: 0,
            max: 255,
            step: 16,
            unit: "",
        });
        e.step(1);
        assert_eq!(e.value(), "255");
        e.step(1);
        assert_eq!(e.value(), "255", "brightness wrapped past full");
        for _ in 0..40 {
            e.step(-1);
        }
        assert_eq!(e.value(), "0");
    }

    /// Text and read-only rows are not stepped; left and right do nothing on
    /// them rather than half-changing something.
    #[test]
    fn text_and_read_only_do_not_step() {
        let mut t = entry(Kind::Text {
            value: "x".into(),
            secret: false,
        });
        assert_eq!(t.step(1), None);
        assert!(!t.steps());
        let mut r = entry(Kind::ReadOnly("/userdata/roms".into()));
        assert_eq!(r.step(1), None);
        assert!(!r.steps());
    }

    /// A secret is never shown, and an empty field says so rather than looking
    /// like a blank value somebody set on purpose.
    #[test]
    fn a_secret_is_dots_and_an_empty_field_says_so() {
        assert_eq!(
            entry(Kind::Text {
                value: String::new(),
                secret: true
            })
            .value(),
            "not set"
        );
        let e = entry(Kind::Text {
            value: "abcd".into(),
            secret: true,
        });
        assert_eq!(e.value(), "\u{2022}\u{2022}\u{2022}\u{2022}");
        // Long secrets do not draw a hundred dots across the pane.
        let long = entry(Kind::Text {
            value: "x".repeat(64),
            secret: true,
        });
        assert_eq!(long.value().chars().count(), 12);
    }

    /// Every field either names a `table.key` the writer can reach, or is
    /// empty. A typo here writes nothing and reports success, which is the
    /// quietest way for a settings screen to be broken.
    #[test]
    fn every_field_is_writable_or_deliberately_not() {
        for pane in built() {
            for e in pane.entries {
                if e.field.is_empty() {
                    assert!(
                        !e.steps() || matches!(e.kind, Kind::Number { .. }),
                        "{} has no field but is editable as a config value",
                        e.label
                    );
                    continue;
                }
                assert!(
                    e.field.split_once('.').is_some(),
                    "{} names {:?}, which is not table.key",
                    e.label,
                    e.field
                );
            }
        }
    }

    /// The panes the handheld has, and not the ones it does not. Window
    /// arrangement, the title bar and fit-to-window are desktop questions.
    #[test]
    fn no_pane_offers_a_setting_the_handheld_cannot_honour() {
        let labels: Vec<String> = built()
            .iter()
            .flat_map(|p| p.entries.iter().map(|e| e.label.to_lowercase()))
            .collect();
        for absent in ["window", "title bar", "fit to the game"] {
            assert!(
                !labels.iter().any(|l| l.contains(absent)),
                "{absent:?} means nothing on a device with no windows"
            );
        }
    }

    /// `device.*` is the hardware and must never reach the config file.
    ///
    /// Backlight in a file is the wrong place twice over: it has to take effect
    /// while the button is held, and the device already remembers it across a
    /// reboot. A row that wrote it to TOML would look like it worked and change
    /// nothing on screen.
    #[test]
    fn device_settings_do_not_go_to_the_config_file() {
        // No backlight on the machine the tests run on, so this reports that
        // rather than writing `[device] brightness` into romm-sdl.toml.
        let err = write("device.brightness", &Written::Number(100)).unwrap_err();
        assert!(
            format!("{err:#}").contains("backlight"),
            "expected a hardware error, got: {err:#}"
        );
        assert!(
            !std::path::Path::new(FILE).exists() || {
                let raw = std::fs::read_to_string(FILE).unwrap_or_default();
                !raw.contains("[device]")
            }
        );
    }

    /// A hardware key nothing knows how to apply is an error, not a silent
    /// success.
    #[test]
    fn an_unknown_device_setting_is_refused() {
        assert!(write("device.nonsense", &Written::Number(1)).is_err());
        assert!(write("device.brightness", &Written::Bool(true)).is_err());
    }

    /// A console with a real choice of cores gets a row; one with a single
    /// core does not.
    ///
    /// A row that cannot change is noise on a screen you came to to change
    /// something.
    #[test]
    fn emulators_offers_only_consoles_with_a_choice() {
        let emu = built()
            .into_iter()
            .find(|p| p.id == "emulators")
            .expect("an Emulators pane");
        assert!(!emu.entries.is_empty(), "no console offered a core choice");
        for e in &emu.entries {
            let Kind::Choice { options, .. } = &e.kind else {
                panic!("{} is not a choice", e.label);
            };
            assert!(
                options.len() >= 2,
                "{} has one option and should not be a row",
                e.label
            );
            assert!(
                e.field.starts_with("cores_overrides."),
                "{} writes to {:?}, which is not the overrides table",
                e.label,
                e.field
            );
        }
    }

    /// The sub-table is spelled back out before anything is written.
    ///
    /// Checked on the translation rather than by calling `write`, which would
    /// create a real config file while the tests run.
    #[test]
    fn the_overrides_sub_table_is_spelled_back_out() {
        assert_eq!(toml_table("cores_overrides"), "cores.overrides");
        assert_eq!(toml_table("library"), "library");
        assert_eq!(toml_table("server"), "server");
    }

    /// Controls offers pad buttons only, and every row can be cleared.
    ///
    /// No keyboard column: a handheld has no keyboard, and 29 rows of the same
    /// dash is a column that only takes room.
    #[test]
    fn controls_are_pad_only_and_can_be_unbound() {
        let pane = built()
            .into_iter()
            .find(|p| p.id == "controls")
            .expect("a Controls pane");
        assert!(!pane.entries.is_empty());
        for e in &pane.entries {
            let Kind::Choice { options, .. } = &e.kind else {
                panic!("{} is not a choice", e.label);
            };
            assert_eq!(options[0].0, "", "{} cannot be unbound", e.label);
            assert_eq!(options[0].1, "not bound");
            assert!(
                e.field.starts_with("bindings_pad."),
                "{} writes to {:?}",
                e.label,
                e.field
            );
        }
    }

    /// The button a binding is on comes back as the option that is selected.
    /// Off by one here and every row opens showing the wrong button.
    #[test]
    fn a_bound_button_selects_its_own_option() {
        let opts = pad_options();
        assert_eq!(pad_index(None), 0, "unbound is the first option");
        for b in romm_desktop::binds::PAD_BUTTONS {
            let at = pad_index(Some(b.index));
            assert_eq!(
                opts[at].0,
                b.index.to_string(),
                "button {} selected the wrong option",
                b.index
            );
        }
    }

    /// This front end never writes the desktop's file.
    ///
    /// On a development Mac both run from the same directory, and before the
    /// split a value stepped in the preview edited the real desktop config one
    /// press at a time with nothing said.
    #[test]
    fn settings_are_written_to_this_front_ends_own_file() {
        assert_ne!(FILE, "config.toml");
        assert!(
            FILE.ends_with(".toml"),
            "still a TOML file, just a different one"
        );
    }

    /// The writer refuses a field it cannot place, rather than writing it into
    /// some default table.
    #[test]
    fn the_writer_refuses_a_field_with_no_table() {
        assert!(write("nodot", &Written::Bool(true)).is_err());
    }
}
