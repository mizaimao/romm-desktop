//! Every patch, as data.
//!
//! The bodies and the files are compiled in. That is the whole point of the
//! addon: *"even on a newly installed KNULLI we can easily configure and
//! recover all of those customized settings"* — which only works if recovery
//! is one binary and one profile, not a binary plus a directory of assets to
//! remember.
//!
//! Each entry's `detail` names the file it writes and says why that file and
//! not the obvious one. Every one of these was placed by hand first, and the
//! hard part was never the change.

use crate::patch::{Choice, Patch, Paths, Step};

const HOTKEYS: &str = include_str!("../assets/hotkeys.conf");
const SHADERS_LCD: &str = include_str!("../assets/shaders.conf");
const SHADERS_PLAIN: &str = include_str!("../assets/shaders-plain.conf");
const SHADERS_ZFAST: &str = include_str!("../assets/shaders-zfast.conf");
const POWER: &str = include_str!("../assets/power.conf");
const BEZEL_GBA: &str = include_str!("../assets/bezel-gba.conf");
const BEZEL_KNULLI: &str = include_str!("../assets/bezel-knulli.conf");
const WIFI_AWAKE: &str = include_str!("../assets/wifi-awake.sh");

const SHADER_1: &[u8] = include_bytes!("../../device/retroarch-shaders/1-sharp-shimmerless.glslp");
const SHADER_2: &[u8] =
    include_bytes!("../../device/retroarch-shaders/2-sharp-shimmerless-scanlines.glslp");
const SHADER_3: &[u8] = include_bytes!("../../device/retroarch-shaders/3-sharp-shimmerless-lcd.glslp");

const BEZEL_PNG: &[u8] = include_bytes!("../../device/gba-bezel/systems/gba-4_3.png");
const BEZEL_INFO: &[u8] = include_bytes!("../../device/gba-bezel/systems/gba-4_3.info");

const BLANK_LOGO: &[u8] = include_bytes!("../../device/splash/blank-logo.png");
const BOOT_HOOK: &[u8] = include_bytes!("../../device/splash/boot-custom.sh");
const ES_INPUT: &[u8] = include_bytes!("../../device/hotkey/es_input.cfg");
const TRIGGERS: &str = include_str!("../../device/hotkey/multimedia_keys.append");

/// Clears the framebuffer, where `S03system-splash` leaves the KNULLI logo.
const CLEAR_FB: &str = r#"case "$1" in
  start)
    if [ -e /dev/fb0 ] && [ -r /sys/class/graphics/fb0/virtual_size ]; then
      W=$(cut -d, -f1 /sys/class/graphics/fb0/virtual_size)
      H=$(cut -d, -f2 /sys/class/graphics/fb0/virtual_size)
      dd if=/dev/zero of=/dev/fb0 bs=4096 count=$(( W * H * 4 / 4096 )) 2>/dev/null
    fi
    ;;
esac"#;

fn block(paths: &Paths, id: &str, body: Option<&str>) -> Step {
    Step::Block {
        file: paths.knulli_conf(),
        id: id.into(),
        body: body.map(str::to_string),
        seed: None,
    }
}

fn startup(paths: &Paths, id: &str, body: Option<&str>) -> Step {
    Step::Block {
        file: paths.user_startup(),
        id: id.into(),
        body: body.map(str::to_string),
        seed: None,
    }
}

fn place(path: std::path::PathBuf, bytes: Option<&'static [u8]>) -> Step {
    Step::Place { path, bytes }
}

fn on_off(name_on: &str, on: Vec<Step>, off: Vec<Step>) -> Vec<Choice> {
    vec![
        Choice { name: "off".into(), steps: off },
        Choice { name: name_on.into(), steps: on },
    ]
}

/// The patches tab, in the order it is drawn.
pub fn all(paths: &Paths) -> Vec<Patch> {
    vec![
        Patch {
            id: "hotkeys",
            title: "Hotkeys",
            detail: "Writes the RetroArch hotkey block into knulli.conf. It has to go there \
                     rather than into RetroArch's own menu, because configgen rewrites \
                     retroarch.cfg at every single launch — which is why changes made inside \
                     RetroArch never stick.",
            choices: on_off(
                "ON",
                vec![block(paths, "hotkeys", Some(HOTKEYS))],
                vec![block(paths, "hotkeys", None)],
            ),
        },
        Patch {
            id: "shaders",
            title: "Shaders",
            detail: "global.shaderset in knulli.conf, plus three presets in \
                     /userdata/shaders/moose that Hotkey + D-pad up/down cycles. \
                     sharp-shimmerless keeps the pixel grid even when the scale factor is not \
                     a whole number, which at 640x480 is nearly always. The preset directory \
                     is needed because configgen otherwise points the cycler at the whole \
                     700-preset library.",
            choices: vec![
                Choice {
                    name: "off".into(),
                    steps: vec![
                        block(paths, "shaders", None),
                        place(paths.shader("1-sharp-shimmerless.glslp"), None),
                        place(paths.shader("2-sharp-shimmerless-scanlines.glslp"), None),
                        place(paths.shader("3-sharp-shimmerless-lcd.glslp"), None),
                    ],
                },
                Choice {
                    name: "shimmerless + LCD/CRT".into(),
                    steps: vec![
                        block(paths, "shaders", Some(SHADERS_LCD)),
                        place(paths.shader("1-sharp-shimmerless.glslp"), Some(SHADER_1)),
                        place(paths.shader("2-sharp-shimmerless-scanlines.glslp"), Some(SHADER_2)),
                        place(paths.shader("3-sharp-shimmerless-lcd.glslp"), Some(SHADER_3)),
                    ],
                },
                Choice {
                    name: "shimmerless plain".into(),
                    steps: vec![
                        block(paths, "shaders", Some(SHADERS_PLAIN)),
                        place(paths.shader("1-sharp-shimmerless.glslp"), Some(SHADER_1)),
                        place(paths.shader("2-sharp-shimmerless-scanlines.glslp"), Some(SHADER_2)),
                        place(paths.shader("3-sharp-shimmerless-lcd.glslp"), Some(SHADER_3)),
                    ],
                },
                Choice {
                    name: "zfast".into(),
                    steps: vec![
                        block(paths, "shaders", Some(SHADERS_ZFAST)),
                        place(paths.shader("1-sharp-shimmerless.glslp"), Some(SHADER_1)),
                        place(paths.shader("2-sharp-shimmerless-scanlines.glslp"), Some(SHADER_2)),
                        place(paths.shader("3-sharp-shimmerless-lcd.glslp"), Some(SHADER_3)),
                    ],
                },
            ],
        },
        Patch {
            id: "bezels",
            title: "Bezels",
            detail: "<system>.bezel in knulli.conf, with artwork in /userdata/decorations — \
                     the shipped packs live on the squashfs and do not survive an upgrade. On \
                     a 4:3 640x480 screen a bezel only earns its place on the handhelds, \
                     whose picture is narrower than the screen anyway.",
            choices: vec![
                Choice {
                    name: "off".into(),
                    steps: vec![
                        block(paths, "bezels", None),
                        place(paths.decoration("gba-4_3.png"), None),
                        place(paths.decoration("gba-4_3.info"), None),
                    ],
                },
                Choice {
                    name: "GBA only".into(),
                    steps: vec![
                        block(paths, "bezels", Some(BEZEL_GBA)),
                        place(paths.decoration("gba-4_3.png"), Some(BEZEL_PNG)),
                        place(paths.decoration("gba-4_3.info"), Some(BEZEL_INFO)),
                    ],
                },
                Choice {
                    name: "KNULLI default".into(),
                    steps: vec![
                        block(paths, "bezels", Some(BEZEL_KNULLI)),
                        place(paths.decoration("gba-4_3.png"), None),
                        place(paths.decoration("gba-4_3.info"), None),
                    ],
                },
            ],
        },
        Patch {
            id: "hotkey-app",
            title: "L2+R2 opens this app",
            detail: "Two lines in /userdata/system/configs/multimedia_keys.conf, which \
                     S50triggerhappy prefers over anything in /etc — and /etc is on the tmpfs \
                     overlay, so a rule there would be gone by the next boot. The file is \
                     seeded from KNULLI's own first, because the /userdata one replaces it \
                     rather than adding to it, and the volume and power keys live in there.",
            choices: on_off(
                "ON",
                vec![Step::Block {
                    file: paths.trigger_conf(),
                    id: "hotkey".into(),
                    body: Some(TRIGGERS.to_string()),
                    seed: Some(paths.stock_triggers()),
                }],
                vec![Step::Block {
                    file: paths.trigger_conf(),
                    id: "hotkey".into(),
                    body: None,
                    seed: Some(paths.stock_triggers()),
                }],
            ),
        },
        Patch {
            id: "es-shoulders",
            title: "ES ignores L2/R2",
            detail: "A 24-line es_input.cfg holding only this device's pad, with l2 and r2 \
                     dropped, so the shoulder combo does not also page the menu about. \
                     Copying KNULLI's file across whole does not work: 291 pad definitions, \
                     and EmulationStation will not start.",
            choices: on_off(
                "ON",
                vec![place(paths.es_input(), Some(ES_INPUT))],
                vec![place(paths.es_input(), None)],
            ),
        },
        Patch {
            id: "es-logo",
            title: "Hide the loading logo",
            detail: "A black 1280x720 PNG over resources/logo.png, put back at every boot by \
                     /boot/boot-custom.sh because /usr is a tmpfs and is stock again each \
                     time. EmulationStation draws that file whenever it is loading — every \
                     game launch and every return from one — and there is no setting for it.",
            choices: on_off(
                "ON",
                vec![
                    place(paths.blank_logo(), Some(BLANK_LOGO)),
                    place(paths.boot_custom(), Some(BOOT_HOOK)),
                    place(paths.es_logo(), Some(BLANK_LOGO)),
                ],
                vec![
                    place(paths.blank_logo(), None),
                    place(paths.es_logo(), None),
                ],
            ),
        },
        Patch {
            id: "boot-splash",
            title: "Clear the boot splash",
            detail: "Zeroes /dev/fb0 from /userdata/system/custom.sh, which S99userservices \
                     runs after S03system-splash has painted the KNULLI logo there and left \
                     it. Only visible when nothing else is drawing — but then it is the whole \
                     screen.",
            choices: on_off(
                "ON",
                vec![startup(paths, "splash", Some(CLEAR_FB))],
                vec![startup(paths, "splash", None)],
            ),
        },
        Patch {
            id: "never-sleep",
            title: "Never sleep",
            detail: "system.batterysaver.extendedmode in knulli.conf. Suspending after 15 \
                     minutes idle drops the network and reads as a dead device. Dimming stays \
                     either way, so the battery is still looked after.",
            choices: on_off(
                "ON",
                vec![block(paths, "power", Some(POWER))],
                vec![block(paths, "power", None)],
            ),
        },
        Patch {
            id: "wifi-awake",
            title: "Keep Wi-Fi awake",
            detail: "Turns off wireless power saving from /userdata/system/custom.sh. Costs \
                     battery; buys a device that answers when you reach for it.",
            choices: on_off(
                "ON",
                vec![startup(paths, "wifi", Some(WIFI_AWAKE))],
                vec![startup(paths, "wifi", None)],
            ),
        },
        Patch {
            id: "gpu",
            title: "Graphics driver",
            detail: "Writes which Mali blob /boot/boot-custom.sh should copy into \
                     /usr/lib at boot. The blobs themselves live in /userdata/system/gpu and \
                     are far too big to carry in here — without them this setting is \
                     remembered but does nothing. The stock one has no Wayland support; the \
                     g24p0 one does, and the emulators behave identically on both.",
            choices: vec![
                Choice {
                    name: "stock".into(),
                    steps: vec![
                        place(paths.gpu_choice(), Some(b"stock\n")),
                        place(paths.boot_custom(), Some(BOOT_HOOK)),
                    ],
                },
                Choice {
                    name: "wayland".into(),
                    steps: vec![
                        place(paths.gpu_choice(), Some(b"wayland\n")),
                        place(paths.boot_custom(), Some(BOOT_HOOK)),
                    ],
                },
            ],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::patch::State;

    fn scratch(name: &str) -> Paths {
        let dir = std::env::temp_dir().join(format!("moose-catalogue-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Paths::new(dir)
    }

    #[test]
    fn a_fresh_device_reads_as_off_everywhere_it_can() {
        // Nothing applied, nothing on disk. Every patch with an "off" option
        // should say so rather than "Changed" — otherwise a new install looks
        // like a corrupted one.
        let paths = scratch("fresh");
        for patch in all(&paths) {
            if patch.choices.iter().any(|c| c.name == "off") {
                assert_eq!(
                    patch.state(),
                    State::At(0),
                    "{} should read as off on a bare device",
                    patch.id
                );
            }
        }
    }

    #[test]
    fn every_patch_applies_and_reads_itself_back() {
        // The round trip that matters: after apply, `state` must agree. If it
        // does not, the menu shows a change still queued that has already
        // happened, and pressing A again would run it forever.
        let paths = scratch("roundtrip");
        for patch in all(&paths) {
            for (i, choice) in patch.choices.iter().enumerate() {
                patch.apply(i).unwrap_or_else(|e| {
                    panic!("{} option {} ({}) failed: {e:#}", patch.id, i, choice.name)
                });
                assert_eq!(
                    patch.state(),
                    State::At(i),
                    "{} did not read back as {}",
                    patch.id,
                    choice.name
                );
            }
        }
    }

    #[test]
    fn turning_everything_on_then_off_leaves_no_trace() {
        // A full revert has to be a full revert. Anything left behind is
        // something the next person has to find by hand.
        let paths = scratch("revert");
        let patches = all(&paths);
        for patch in &patches {
            let last = patch.choices.len() - 1;
            patch.apply(last).unwrap();
        }
        for patch in &patches {
            if let Some(off) = patch.choices.iter().position(|c| c.name == "off") {
                patch.apply(off).unwrap();
                assert_eq!(patch.state(), State::At(off), "{} would not revert", patch.id);
            }
        }
        // knulli.conf should be back to nothing of ours.
        let conf = std::fs::read_to_string(paths.knulli_conf()).unwrap_or_default();
        assert!(
            !conf.contains("## moose-patch:"),
            "knulli.conf still holds our blocks:\n{conf}"
        );
    }

    #[test]
    fn the_trigger_file_keeps_knullis_own_keys() {
        // The trap: /userdata/system/configs/multimedia_keys.conf *replaces*
        // the one in /etc. Creating it with only our block would take volume,
        // power and the lid switch away.
        let paths = scratch("triggers");
        let stock = paths.stock_triggers();
        std::fs::create_dir_all(stock.parent().unwrap()).unwrap();
        std::fs::write(&stock, "KEY_VOLUMEUP 1  /usr/bin/volume-button volup\n").unwrap();

        let patch = all(&paths).into_iter().find(|p| p.id == "hotkey-app").unwrap();
        patch.apply(1).unwrap();

        let written = std::fs::read_to_string(paths.trigger_conf()).unwrap();
        assert!(written.contains("KEY_VOLUMEUP"), "volume keys were lost:\n{written}");
        assert!(written.contains("BTN_TR2"));
    }

    #[test]
    fn hiding_the_logo_gives_es_its_own_file_back() {
        // If "off" deleted logo.png instead of restoring it, ES would be
        // missing a resource it loads unconditionally.
        let paths = scratch("logo");
        let logo = paths.es_logo();
        std::fs::create_dir_all(logo.parent().unwrap()).unwrap();
        std::fs::write(&logo, b"the real KNULLI beetle").unwrap();

        let patch = all(&paths).into_iter().find(|p| p.id == "es-logo").unwrap();
        patch.apply(1).unwrap();
        assert_eq!(std::fs::read(&logo).unwrap(), BLANK_LOGO);
        patch.apply(0).unwrap();
        assert_eq!(std::fs::read(&logo).unwrap(), b"the real KNULLI beetle");
    }

    #[test]
    fn two_patches_sharing_one_file_do_not_tread_on_each_other() {
        // never-sleep and hotkeys both live in knulli.conf; boot-splash and
        // wifi-awake both live in custom.sh. Applying one must not disturb
        // the other's block.
        let paths = scratch("shared");
        let patches = all(&paths);
        let get = |id: &str| patches.iter().find(|p| p.id == id).unwrap();

        get("hotkeys").apply(1).unwrap();
        get("never-sleep").apply(1).unwrap();
        assert_eq!(get("hotkeys").state(), State::At(1), "sleep clobbered hotkeys");

        get("boot-splash").apply(1).unwrap();
        get("wifi-awake").apply(1).unwrap();
        assert_eq!(get("boot-splash").state(), State::At(1), "wifi clobbered splash");

        // And taking one away leaves the other.
        get("never-sleep").apply(0).unwrap();
        assert_eq!(get("hotkeys").state(), State::At(1));
    }
}
