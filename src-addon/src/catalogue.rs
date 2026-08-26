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
const WIFI_AWAKE: &str = include_str!("../assets/wifi-awake.sh");

const SHADER_1: &[u8] = include_bytes!("../../device/retroarch-shaders/1-sharp-shimmerless.glslp");
const SHADER_2: &[u8] =
    include_bytes!("../../device/retroarch-shaders/2-sharp-shimmerless-scanlines.glslp");
const SHADER_3: &[u8] = include_bytes!("../../device/retroarch-shaders/3-sharp-shimmerless-lcd.glslp");
const SHADER_4: &[u8] = include_bytes!("../../device/retroarch-shaders/4-zfast-crt.glslp");

const SET_LCD: &[u8] = include_bytes!("../assets/shadersets/lcd.yml");
const SET_PLAIN: &[u8] = include_bytes!("../assets/shadersets/plain.yml");
const SET_ZFAST: &[u8] = include_bytes!("../assets/shadersets/zfast.yml");

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

fn place(paths: &Paths, path: std::path::PathBuf, bytes: Option<&'static [u8]>) -> Step {
    let backup = paths.backup_for(&path);
    Step::Place { path, bytes, backup }
}

/// The four presets, plus whichever set is being chosen. Every option lays
/// down all four, so the cycle is the same list whichever one you start from.
fn shader_files(paths: &Paths, set: Option<(&str, &'static [u8])>) -> Vec<Step> {
    let presets: [(&str, Option<&'static [u8]>); 4] = match set {
        Some(_) => [
            ("1-sharp-shimmerless.glslp", Some(SHADER_1)),
            ("2-sharp-shimmerless-scanlines.glslp", Some(SHADER_2)),
            ("3-sharp-shimmerless-lcd.glslp", Some(SHADER_3)),
            ("4-zfast-crt.glslp", Some(SHADER_4)),
        ],
        None => [
            ("1-sharp-shimmerless.glslp", None),
            ("2-sharp-shimmerless-scanlines.glslp", None),
            ("3-sharp-shimmerless-lcd.glslp", None),
            ("4-zfast-crt.glslp", None),
        ],
    };
    let mut steps: Vec<Step> = presets
        .iter()
        .map(|(name, bytes)| place(paths, paths.shader(name), *bytes))
        .collect();
    for (name, body) in [
        ("moose-lcd", SET_LCD),
        ("moose-plain", SET_PLAIN),
        ("moose-zfast", SET_ZFAST),
    ] {
        steps.push(place(paths, paths.shaderset(name), set.map(|_| body)));
    }
    steps
}

fn on_off(name_on: &str, on: Vec<Step>, off: Vec<Step>) -> Vec<Choice> {
    vec![
        Choice { name: "off".into(), steps: off },
        Choice { name: name_on.into(), steps: on },
    ]
}

/// One system's bezel. `ours` is whether we carry artwork for it — only GBA,
/// so far, and only because the stock one is a washed-out khaki.
fn bezel(paths: &Paths, slug: &'static str, name: &'static str, ours: bool) -> Patch {
    let id: &'static str = Box::leak(format!("bezel-{slug}").into_boxed_str());
    let title: &'static str = Box::leak(format!("{name} bezel").into_boxed_str());
    let detail: &'static str = Box::leak(
        format!(
            "{slug}.bezel in knulli.conf. KNULLI's own artwork lives on the squashfs and \
             does not survive an upgrade; ours goes in /userdata/decorations, which does. \
             The game keeps the full width either way — the border only fills the strip \
             this system's picture leaves empty on a 4:3 screen."
        )
        .into_boxed_str(),
    );

    let mut choices = vec![
        Choice {
            name: "off".into(),
            steps: vec![
                block(paths, &format!("bezel-{slug}"), None),
                place(paths, paths.decoration(&format!("{slug}-4_3.png")), None),
                place(paths, paths.decoration(&format!("{slug}-4_3.info")), None),
            ],
        },
        Choice {
            name: "KNULLI".into(),
            steps: vec![
                block(paths, &format!("bezel-{slug}"), Some(&format!("{slug}.bezel=default-knulli"))),
                place(paths, paths.decoration(&format!("{slug}-4_3.png")), None),
                place(paths, paths.decoration(&format!("{slug}-4_3.info")), None),
            ],
        },
    ];
    if ours {
        choices.push(Choice {
            name: "silver".into(),
            steps: vec![
                block(paths, &format!("bezel-{slug}"), Some(&format!("{slug}.bezel=moose"))),
                place(paths, paths.decoration(&format!("{slug}-4_3.png")), Some(BEZEL_PNG)),
                place(paths, paths.decoration(&format!("{slug}-4_3.info")), Some(BEZEL_INFO)),
            ],
        });
    }
    Patch { id, title, detail, choices }
}

/// One system's shader, over the global set.
fn shader(paths: &Paths, slug: &'static str, name: &'static str) -> Patch {
    let id: &'static str = Box::leak(format!("shader-{slug}").into_boxed_str());
    let title: &'static str = Box::leak(format!("{name} shader").into_boxed_str());
    let detail: &'static str = Box::leak(
        format!(
            "{slug}.shaderset in knulli.conf, which wins over the global one. Worth setting \
             when a bezel already draws an LCD grid, because two grids stacked look dirty. \
             Left at 'follow global' there is no {slug} line in the file at all."
        )
        .into_boxed_str(),
    );
    Patch {
        id,
        title,
        detail,
        choices: vec![
            Choice {
                name: "follow global".into(),
                steps: vec![block(paths, &format!("shader-{slug}"), None)],
            },
            Choice {
                name: "shimmerless plain".into(),
                steps: vec![block(
                    paths,
                    &format!("shader-{slug}"),
                    Some(&format!("{slug}.shaderset=moose-plain")),
                )],
            },
            Choice {
                name: "shimmerless + LCD".into(),
                steps: vec![block(
                    paths,
                    &format!("shader-{slug}"),
                    Some(&format!("{slug}.shaderset=moose-lcd")),
                )],
            },
            Choice {
                name: "none".into(),
                steps: vec![block(
                    paths,
                    &format!("shader-{slug}"),
                    Some(&format!("{slug}.shaderset=none")),
                )],
            },
        ],
    }
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
            detail: "global.shaderset in knulli.conf, pointed at our own set in \
                     /userdata/shaders. That is not vanity: configgen resolves a set's \
                     presets relative to that directory, and RetroArch cycles the folder of \
                     the preset it loaded — so a stock set makes Hotkey + D-pad walk all \
                     seven hundred presets in the library, most of which this handheld \
                     cannot afford. Ours holds four, all cheap.",
            choices: vec![
                Choice {
                    name: "off".into(),
                    steps: {
                        let mut s = vec![block(paths, "shaders", None)];
                        s.extend(shader_files(paths, None));
                        s
                    },
                },
                Choice {
                    name: "shimmerless + LCD/CRT".into(),
                    steps: {
                        let mut s = vec![block(paths, "shaders", Some(SHADERS_LCD))];
                        s.extend(shader_files(paths, Some(("moose-lcd", SET_LCD))));
                        s
                    },
                },
                Choice {
                    name: "shimmerless plain".into(),
                    steps: {
                        let mut s = vec![block(paths, "shaders", Some(SHADERS_PLAIN))];
                        s.extend(shader_files(paths, Some(("moose-plain", SET_PLAIN))));
                        s
                    },
                },
                Choice {
                    name: "zfast".into(),
                    steps: {
                        let mut s = vec![block(paths, "shaders", Some(SHADERS_ZFAST))];
                        s.extend(shader_files(paths, Some(("moose-zfast", SET_ZFAST))));
                        s
                    },
                },
            ],
        },
        // Bezels, one system at a time.
        //
        // Only these three have 4:3 artwork, and that is not an oversight in
        // KNULLI: a 4:3 console game already fills a 4:3 screen, so there is
        // nowhere for a border to go that is not the picture. The handhelds
        // are narrower than the screen and have real space going spare.
        bezel(paths, "gba", "Game Boy Advance", true),
        bezel(paths, "gb", "Game Boy", false),
        bezel(paths, "gbc", "Game Boy Color", false),

        // And the shader, per system, over the global one.
        //
        // Not `<system>.smooth` — knulli.conf says of it, in its own words,
        // "Is overidden if using a shader set", so a bilinear-filter row would
        // do nothing at all while any shader set is on.
        shader(paths, "gba", "Game Boy Advance"),
        shader(paths, "gb", "Game Boy"),
        shader(paths, "gbc", "Game Boy Color"),

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
                vec![place(paths, paths.es_input(), Some(ES_INPUT))],
                vec![place(paths, paths.es_input(), None)],
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
                    place(paths, paths.blank_logo(), Some(BLANK_LOGO)),
                    place(paths, paths.boot_custom(), Some(BOOT_HOOK)),
                    place(paths, paths.es_logo(), Some(BLANK_LOGO)),
                ],
                vec![
                    place(paths, paths.blank_logo(), None),
                    place(paths, paths.es_logo(), None),
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
                    // No marker file at all, rather than one saying "stock".
                    // The hook does nothing without it, so this is what a
                    // device that has never been touched looks like — and a
                    // fresh install must not read as "changed".
                    name: "stock".into(),
                    steps: vec![place(paths, paths.gpu_choice(), None)],
                },
                Choice {
                    name: "wayland".into(),
                    steps: vec![
                        place(paths, paths.gpu_choice(), Some(b"wayland\n")),
                        place(paths, paths.boot_custom(), Some(BOOT_HOOK)),
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
    fn a_fresh_device_reads_as_the_first_option_everywhere() {
        // The convention every other part of this leans on: **option 0 is the
        // one that touches nothing**. "off" for most, "follow global" for the
        // per-system shaders, "stock" for the driver. A bare device must read
        // as that, or a new install looks like a corrupted one and `--restore`
        // has nothing sane to fall back to.
        let paths = scratch("fresh");
        for patch in all(&paths) {
            assert_eq!(
                patch.state(),
                State::At(0),
                "{} should read as '{}' on a bare device",
                patch.id,
                patch.choices[0].name
            );
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
    fn turning_everything_on_then_back_leaves_no_trace() {
        // A full revert has to be a full revert. Anything left behind is
        // something the next person has to find by hand — and knulli.conf is
        // read last-wins, so a stray block is a setting still in force.
        let paths = scratch("revert");
        let patches = all(&paths);
        for patch in &patches {
            patch.apply(patch.choices.len() - 1).unwrap();
        }
        for patch in &patches {
            patch.apply(0).unwrap();
            assert_eq!(patch.state(), State::At(0), "{} would not go back", patch.id);
        }
        let conf = std::fs::read_to_string(paths.knulli_conf()).unwrap_or_default();
        assert!(
            !conf.contains("## moose-patch:"),
            "knulli.conf still holds our blocks:\n{conf}"
        );
        let startup = std::fs::read_to_string(paths.user_startup()).unwrap_or_default();
        assert!(
            !startup.contains("## moose-patch:"),
            "custom.sh still holds our blocks:\n{startup}"
        );
    }

    #[test]
    fn no_two_patches_write_the_same_block() {
        // Sharing a marker would mean one patch silently overwriting the
        // other's settings, and both reporting the same state. Within a single
        // patch the repeats are the point — that is how "off" undoes "on" —
        // so the check is per (file, block) across *different* patches.
        let paths = scratch("unique");
        let mut ids = std::collections::BTreeSet::new();
        let mut owner: std::collections::BTreeMap<(std::path::PathBuf, String), &str> =
            Default::default();

        for patch in all(&paths) {
            assert!(ids.insert(patch.id.to_string()), "duplicate patch id {}", patch.id);
            for choice in &patch.choices {
                for step in &choice.steps {
                    if let crate::patch::Step::Block { file, id, .. } = step {
                        let key = (file.clone(), id.clone());
                        match owner.get(&key) {
                            Some(first) if *first != patch.id => panic!(
                                "{} and {first} both write block '{id}' in {}",
                                patch.id,
                                file.display()
                            ),
                            _ => {
                                owner.insert(key, patch.id);
                            }
                        }
                    }
                }
            }
        }
        assert!(owner.len() >= 12, "expected a block per config patch, saw {}", owner.len());
    }

    #[test]
    fn every_shader_option_keeps_the_cycle_inside_our_folder() {
        // RetroArch cycles the directory of the preset it loaded. Name a stock
        // set here and Hotkey + D-pad walks the whole library — seven hundred
        // presets, most of them far too heavy for this handheld. That is a
        // regression you cannot see from the code, only from the device, so
        // it gets a test.
        let paths = scratch("shader-cycle");
        for patch in all(&paths) {
            if !(patch.id == "shaders" || patch.id.starts_with("shader-")) {
                continue;
            }
            for choice in &patch.choices {
                for step in &choice.steps {
                    let crate::patch::Step::Block { body: Some(body), .. } = step else {
                        continue;
                    };
                    for line in body.lines() {
                        let Some((_, set)) = line.trim().split_once("shaderset=") else {
                            continue;
                        };
                        assert!(
                            set == "none" || set.starts_with("moose-"),
                            "{} / {} points at '{set}', which lives in the stock library",
                            patch.id,
                            choice.name
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn choosing_a_shader_lays_down_the_whole_cycle() {
        // All four presets, whichever option you pick, so the list you cycle
        // is the same list every time. And "off" takes them all away again.
        let paths = scratch("shader-files");
        let shaders = all(&paths).into_iter().find(|p| p.id == "shaders").unwrap();

        shaders.apply(1).unwrap();
        let present = |n: &str| paths.shader(n).exists();
        assert!(present("1-sharp-shimmerless.glslp"));
        assert!(present("4-zfast-crt.glslp"));
        assert!(paths.shaderset("moose-lcd").exists(), "the set itself must be written");

        shaders.apply(0).unwrap();
        assert!(!present("1-sharp-shimmerless.glslp"), "off leaves nothing behind");
        assert!(!paths.shaderset("moose-lcd").exists());
    }

    #[test]
    fn only_the_systems_with_artwork_get_a_bezel_row() {
        // gba, gb and gbc, and no others. A 4:3 console game already fills a
        // 4:3 screen, so a border there costs picture and buys nothing —
        // which is also why KNULLI ships no 4:3 artwork for them.
        let paths = scratch("bezel-rows");
        let rows: Vec<&str> = all(&paths)
            .iter()
            .map(|p| p.id)
            .filter(|id| id.starts_with("bezel-"))
            .map(|id| Box::leak(id.to_string().into_boxed_str()) as &str)
            .collect();
        assert_eq!(rows, vec!["bezel-gba", "bezel-gb", "bezel-gbc"]);
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
