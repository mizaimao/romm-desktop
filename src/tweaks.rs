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
        ("nes" | "famicom", "fceumm") => &[
            ("fceumm_turbo_enable", "Both"),
            // Frames between repeats. 3 is roughly 10 presses/second, fast
            // enough to matter and slow enough that games still register it.
            ("fceumm_turbo_delay", "3"),
        ],
        _ => &[],
    }
}

/// RetroPad button ids, as used in a `.rmp`.
const PAD_Y: &str = "1";
const PAD_X: &str = "9";

/// Input remap lines for a platform, if any.
///
/// FCEUmm binds Turbo B to RetroPad Y and Turbo A to RetroPad X. On an Xbox
/// pad RetroPad Y is the *west* button (X) and RetroPad X is the *north* one
/// (Y), so out of the box X gives rapid B and Y gives rapid A — the opposite
/// of what was asked. Swapping the two fixes it.
///
/// A swap is symmetric, which matters here: RetroArch's `.rmp` files are
/// ambiguous about whether the key is the physical button or the one the core
/// sees. Exchanging a pair gives the same result under either reading, so this
/// needs no assumption about the direction.
pub fn remap(platform: &str, core: &str) -> Vec<String> {
    match (platform, core) {
        ("nes" | "famicom", "fceumm") => (1..=2)
            .flat_map(|p| {
                [
                    format!("input_player{p}_btn_x = \"{PAD_Y}\""),
                    format!("input_player{p}_btn_y = \"{PAD_X}\""),
                ]
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// The directory name RetroArch uses for a core's own settings.
///
/// It is the core's *display* name, not the library stem — `fceumm` keeps its
/// options in `config/FCEUmm/FCEUmm.opt`. Only cores this module actually
/// configures need an entry.
pub fn core_dir_name(core: &str) -> Option<&'static str> {
    match core {
        "fceumm" => Some("FCEUmm"),
        _ => None,
    }
}

/// Human summary of what was applied, for the launch output.
pub fn describe(platform: &str, core: &str) -> Option<String> {
    if core_options(platform, core).is_empty() {
        return None;
    }
    if platform == "nes" || platform == "famicom" {
        return Some("rapid fire: X = rapid A, Y = rapid B (Xbox layout)".to_owned());
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

    /// The redirect is global once enabled, so it must stay off for everything
    /// that does not need it — otherwise every other core starts reading our
    /// options file instead of the user's own.
    #[test]
    fn nothing_is_applied_to_a_platform_that_does_not_need_it() {
        for (platform, core) in [("snes", "snes9x"), ("nes", "mesen"), ("arcade", "fbneo")] {
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
        // The platforms this module knows about, paired with the core each
        // entry is keyed on.
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
}
