//! Per-platform core options and input remaps.
//!
//! Distinct from `shaders.rs` because RetroArch keeps these somewhere else
//! entirely: core options are **not** part of `retroarch.cfg`, so they cannot
//! be delivered through `--appendconfig`. They live in
//! `config/<Core>/<Core>.opt`, and a remap in `config/remaps/<Core>/<Core>.rmp`.
//!
//! Rather than edit those — they are the user's own RetroArch settings — the
//! launch config redirects both locations into the project's `library/`
//! folder:
//!
//! ```text
//! global_core_options      = "true"
//! core_options_path        = <library>/retroarch/core-options.cfg
//! input_remapping_directory = <library>/retroarch/remaps
//! ```
//!
//! Verified against the running emulator: with those set, RetroArch reads and
//! rewrites *our* file and leaves the user's `FCEUmm.opt` byte-identical.
//! `core_options_path` alone is not enough — the per-core file wins unless
//! `global_core_options` is also on.
//!
//! Because the redirect is global once enabled, it is applied **only** for
//! platforms that actually need it, and the user's existing options for that
//! core are copied in first so nothing they set is lost.

/// A core option, as `key = "value"`.
pub type Opt = (&'static str, &'static str);

/// Core options to force for a platform, if any.
///
/// NES rapid fire: FCEUmm exposes real turbo buttons as core options, which is
/// better than RetroArch's own turbo — that offers a single turbo button, and
/// this needs two independent ones.
pub fn core_options(platform: &str, core: &str) -> &'static [Opt] {
    match (platform, core) {
        // MAME shows a disclaimer, then a warnings screen listing every ROM it
        // is unhappy about, and waits on each. On a set this size that is
        // several seconds of reading before every single arcade game. Both
        // screens are informational -- the game either runs or it does not,
        // and the warning does not change which.
        (_, "mame2003_plus") => &[
            ("mame2003-plus_skip_disclaimer", "enabled"),
            ("mame2003-plus_skip_warnings", "enabled"),
        ],
        (_, "mame2003") => &[
            ("mame2003_skip_disclaimer", "enabled"),
            ("mame2003_skip_warnings", "enabled"),
        ],
        ("nes" | "famicom", "fceumm") => &[
            ("fceumm_turbo_enable", "Both"),
            // Frames between repeats. 3 is roughly 10 presses/second, fast
            // enough to matter and slow enough that games still register it.
            ("fceumm_turbo_delay", "3"),
        ],
        // SwanStation asks RetroArch for a Vulkan context, which on macOS means
        // MoltenVK, and the GPU device is lost within a second or two of the
        // game starting: the boot logo draws, the device dies, and the picture
        // never advances. It looks exactly like a game that will not load, and
        // the log is the only place it says otherwise
        // (VK_ERROR_DEVICE_LOST, over and over).
        //
        // OpenGL is still a hardware renderer -- upscaling and the rest are
        // unaffected -- and it survives. Only on macOS: the Vulkan path is the
        // better one everywhere it works.
        #[cfg(target_os = "macos")]
        (_, "swanstation") => &[("swanstation_GPU_Renderer", "OpenGL")],
        // The rest have their rapid-fire buttons on always; the option only
        // sets how fast they repeat. Left alone the default is slower than
        // anyone wants from a button labelled turbo.
        (_, "mesen") => &[("mesen_controllerturbospeed", "Fast")],
        (_, "gambatte") => &[("gambatte_turbo_period", "4")],
        (_, "gpsp") => &[("gpsp_turbo_period", "4")],
        _ => &[],
    }
}

/// RetroPad button ids, as used in a `.rmp`.
const PAD_Y: &str = "1";
const PAD_X: &str = "9";

/// Cores whose emulated pad has rapid-fire copies of its face buttons.
///
/// These are the two-button consoles, where the pad has spare buttons that the
/// original hardware never had: the NES had B and A, so an Xbox pad running it
/// has two face buttons doing nothing, and the core hands them back as rapid
/// versions of the two real ones.
///
/// Every core here uses the same convention — **Turbo A on RetroPad X, Turbo B
/// on RetroPad Y** — which was read out of the core binaries themselves rather
/// than assumed. Each one declares its buttons to the frontend as a list of
/// names, and `Turbo A` / `Turbo B` sit in the X and Y slots in all of them.
///
/// Absent on purpose:
///
/// * `snes9x` — the SNES uses all four face buttons, so its rapid-fire is on
///   L2/R2/L3/R3 instead. Those are analog triggers and stick clicks on a
///   modern pad, and the core's own note says brushing one mid-game engages
///   rapid fire and breaks anything that needs a button held down.
/// * `genesis_plus_gx` — no rapid fire of any kind, on any of the four
///   consoles it emulates.
/// * `mednafen_pce` — has it, but as an option that converts button I into an
///   autofire button rather than adding a separate one, which costs you the
///   normal press.
const TURBO_CORES: &[&str] =
    &["fceumm", "mesen", "nestopia", "gambatte", "mgba", "gpsp"];

/// Input remap lines for a platform, if any.
///
/// The cores put Turbo B on RetroPad Y and Turbo A on RetroPad X. On an Xbox
/// pad RetroPad Y is the *west* button (X) and RetroPad X is the *north* one
/// (Y), so out of the box X gives rapid B and Y gives rapid A — the opposite
/// of what was asked. Swapping the two fixes it.
///
/// A swap is symmetric, which matters here: RetroArch's `.rmp` files are
/// ambiguous about whether the key is the physical button or the one the core
/// sees. Exchanging a pair gives the same result under either reading, so this
/// needs no assumption about the direction.
pub fn remap(_platform: &str, core: &str) -> Vec<String> {
    if !TURBO_CORES.contains(&core) {
        return Vec::new();
    }
    (1..=2)
        .flat_map(|p| {
            [
                format!("input_player{p}_btn_x = \"{PAD_Y}\""),
                format!("input_player{p}_btn_y = \"{PAD_X}\""),
            ]
        })
        .collect()
}

/// The directory name RetroArch uses for a core's own settings.
///
/// It is the core's *display* name, not the library stem — `fceumm` keeps its
/// options in `config/FCEUmm/FCEUmm.opt`. Only cores this module actually
/// configures need an entry.
pub fn core_dir_name(core: &str) -> Option<&'static str> {
    match core {
        "fceumm" => Some("FCEUmm"),
        "mesen" => Some("Mesen"),
        "nestopia" => Some("Nestopia"),
        "gambatte" => Some("Gambatte"),
        "mgba" => Some("mGBA"),
        "gpsp" => Some("gpSP"),
        "mame2003_plus" => Some("MAME 2003-Plus"),
        "mame2003" => Some("MAME 2003"),
        "swanstation" => Some("SwanStation"),
        _ => None,
    }
}

/// Human summary of what was applied, for the launch output.
pub fn describe(platform: &str, core: &str) -> Option<String> {
    if TURBO_CORES.contains(&core) {
        return Some("rapid fire: X = rapid A, Y = rapid B (Xbox layout)".to_owned());
    }
    if core_options(platform, core).is_empty() {
        return None;
    }
    if core.starts_with("mame2003") {
        return Some("skipping the MAME disclaimer and warning screens".to_owned());
    }
    Some(format!("{} core options applied", core_options(platform, core).len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every platform this project treats as NES has to get the same treatment,
    /// or rapid fire works on one of them and silently not the other.
    #[test]
    fn both_nes_platforms_get_the_turbo_options() {
        for platform in ["nes", "famicom"] {
            let opts = core_options(platform, "fceumm");
            assert_eq!(opts.len(), 2, "{platform}");
            assert!(opts.contains(&("fceumm_turbo_enable", "Both")), "{platform}");
            assert_eq!(remap(platform, "fceumm").len(), 4, "two buttons, two players");
            assert!(describe(platform, "fceumm").is_some());
        }
    }

    /// The consoles with two buttons all have to get rapid fire, whichever
    /// core they are set to. Getting this right on the NES and not the Game Boy
    /// is the failure that looks like the feature works.
    #[test]
    fn every_two_button_console_gets_rapid_fire() {
        for (platform, core) in [
            ("nes", "fceumm"),
            ("famicom", "mesen"),
            ("nes", "nestopia"),
            ("gb", "gambatte"),
            ("gbc", "gambatte"),
            ("gba", "mgba"),
            ("gba", "gpsp"),
        ] {
            assert_eq!(
                remap(platform, core).len(),
                4,
                "{platform}/{core}: two buttons swapped, for two players"
            );
            assert!(describe(platform, core).is_some(), "{platform}/{core}");
        }
    }

    /// The redirect is global once enabled, so it must stay off for everything
    /// that does not need it — otherwise every other core starts reading our
    /// options file instead of the user's own.
    #[test]
    fn nothing_is_applied_to_a_platform_that_does_not_need_it() {
        // snes9x and genesis_plus_gx are the deliberate omissions from
        // TURBO_CORES — see the note there — so they are the honest check that
        // the redirect stays off rather than a core nobody has looked at.
        for (platform, core) in
            [("snes", "snes9x"), ("genesis", "genesis_plus_gx"), ("arcade", "fbneo")]
        {
            assert!(core_options(platform, core).is_empty(), "{platform}/{core}");
            assert!(remap(platform, core).is_empty(), "{platform}/{core}");
            assert!(describe(platform, core).is_none(), "{platform}/{core}");
        }
    }

    /// The trap this guards: `prepare_tweaks` gives up silently when a core has
    /// no directory name, so options defined without one are written nowhere
    /// and the feature just does not happen. Adding a core to `core_options`
    /// and forgetting `core_dir_name` must fail here rather than in play.
    #[test]
    fn every_core_with_options_also_has_a_directory_name() {
        // Every core that gets rapid fire, not a hand-listed few: adding one
        // to TURBO_CORES and forgetting its directory name is exactly the
        // mistake this is here to catch.
        for core in TURBO_CORES {
            assert!(
                !remap("", core).is_empty(),
                "{core} is listed as a turbo core but emits no remap"
            );
            assert!(
                core_dir_name(core).is_some(),
                "{core} has settings to write but no config directory name, \
                 so prepare_tweaks would discard them"
            );
        }
        for (platform, core) in [("nes", "fceumm"), ("famicom", "fceumm")] {
            if !core_options(platform, core).is_empty() || !remap(platform, core).is_empty() {
                assert!(
                    core_dir_name(core).is_some(),
                    "{core} has settings to write but no config directory name, \
                     so prepare_tweaks would discard them"
                );
            }
        }
        assert_eq!(core_dir_name("fceumm"), Some("FCEUmm"), "display name, not the stem");
        assert_eq!(core_dir_name("snes9x"), None);
    }

    /// The remap is a swap, and that is what makes it correct under either
    /// reading of a .rmp file. If both keys ever pointed at the same button it
    /// would stop being symmetric and the direction would start to matter.
    #[test]
    fn the_turbo_remap_exchanges_the_two_buttons() {
        let lines = remap("nes", "fceumm");
        for player in 1..=2 {
            assert!(
                lines.contains(&format!("input_player{player}_btn_x = \"{PAD_Y}\"")),
                "player {player} X takes Y's id"
            );
            assert!(
                lines.contains(&format!("input_player{player}_btn_y = \"{PAD_X}\"")),
                "player {player} Y takes X's id"
            );
        }
        assert_ne!(PAD_X, PAD_Y, "a swap between equal ids would be a no-op");
    }

    /// Every emitted remap line has to parse as a RetroArch assignment; a
    /// malformed one is ignored silently and the buttons stay swapped.
    #[test]
    fn remap_lines_are_well_formed_assignments() {
        for line in remap("nes", "fceumm") {
            let (key, value) = line.split_once(" = ").expect("key = value");
            assert!(key.starts_with("input_player"), "unexpected key: {key}");
            assert!(value.starts_with('"') && value.ends_with('"'), "unquoted: {line}");
            assert!(
                value.trim_matches('"').chars().all(|c| c.is_ascii_digit()),
                "a RetroPad id must be numeric: {line}"
            );
        }
    }

    /// SwanStation asks RetroArch for a Vulkan context, which on macOS is
    /// MoltenVK, and the GPU device is lost a second or two into the game:
    /// 2,397 VK_ERROR_DEVICE_LOST in one run. The boot logo draws, the device
    /// dies, and the picture never advances — which reads as a game that will
    /// not load, with nothing on screen to say otherwise.
    ///
    /// OpenGL is still a hardware renderer, so nothing is given up.
    #[test]
    #[cfg(target_os = "macos")]
    fn playstation_games_are_kept_off_the_vulkan_path_on_macos() {
        let opts = core_options("psx", "swanstation");
        assert!(
            opts.iter().any(|(k, v)| *k == "swanstation_GPU_Renderer" && *v == "OpenGL"),
            "{opts:?}"
        );
        // And the option has somewhere to be written: RetroArch keeps a core's
        // settings under its *display* name, and a core missing from that map
        // has its options silently dropped.
        assert_eq!(core_dir_name("swanstation"), Some("SwanStation"));
    }

}
