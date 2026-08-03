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
