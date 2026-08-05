//! Per-platform video shaders.
//!
//! ES-DE leaves this to RetroArch; we set it per platform at launch instead,
//! so a CRT console gets a CRT mask and a handheld gets its LCD grid without
//! anyone opening RetroArch's menu.
//!
//! Presets are `.slangp` because this install runs the Vulkan driver. A GL
//! build would want `.glslp`; [`available`] checks the file exists before
//! offering it, so a missing format degrades to "no shader" rather than a
//! black screen.

use std::path::PathBuf;

use crate::retroarch::RetroArch;

/// What kind of display the system had — decides which presets make sense.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Display {
    /// Plugged into a television. Wants scanlines, mask, curvature.
    Crt,
    /// Its own LCD panel. Wants a pixel grid, not scanlines.
    Handheld,
}

pub struct ShaderOption {
    /// Path under `shaders_slang/`, without the `.slangp` suffix.
    pub path: &'static str,
    pub label: &'static str,
    pub note: &'static str,
    pub display: Display,
}

/// Curated presets, heaviest-first within each group.
///
/// Deliberately short: RetroArch ships 1,932 presets, and a list that long is
/// a worse experience than a considered dozen.
pub const CATALOGUE: &[ShaderOption] = &[
    // --- CRT ---
    ShaderOption { path: "crt/crt-guest-advanced", label: "CRT — Guest Advanced", note: "Best looking, heaviest", display: Display::Crt },
    ShaderOption { path: "crt/crt-royale", label: "CRT — Royale", note: "Classic reference, heavy", display: Display::Crt },
    ShaderOption { path: "crt/crt-geom", label: "CRT — Geom", note: "Curvature, moderate cost", display: Display::Crt },
    ShaderOption { path: "crt/crt-hyllian", label: "CRT — Hyllian", note: "Sharp, good for 2D", display: Display::Crt },
    ShaderOption { path: "crt/crt-aperture", label: "CRT — Aperture", note: "Aperture grille, light", display: Display::Crt },
    ShaderOption { path: "crt/crt-lottes", label: "CRT — Lottes", note: "Warm, moderate", display: Display::Crt },
    ShaderOption { path: "crt/crt-easymode", label: "CRT — EasyMode", note: "Light and clean", display: Display::Crt },
    ShaderOption { path: "crt/zfast-crt", label: "CRT — zfast", note: "Cheapest scanlines", display: Display::Crt },
    // --- Handheld ---
    ShaderOption { path: "handheld/gameboy", label: "Game Boy DMG", note: "Green dot matrix", display: Display::Handheld },
    ShaderOption { path: "handheld/sameboy-lcd", label: "SameBoy LCD", note: "Accurate DMG panel", display: Display::Handheld },
    ShaderOption { path: "handheld/gameboy-pocket", label: "Game Boy Pocket", note: "Grey panel", display: Display::Handheld },
    ShaderOption { path: "handheld/gameboy-color-dot-matrix", label: "GBC dot matrix", note: "Colour dot matrix", display: Display::Handheld },
    ShaderOption { path: "handheld/agb001", label: "GBA (AGB-001)", note: "Original non-backlit", display: Display::Handheld },
    ShaderOption { path: "handheld/ags001", label: "GBA SP (AGS-001)", note: "Front-lit SP", display: Display::Handheld },
    ShaderOption { path: "handheld/gameboy-advance-dot-matrix", label: "GBA dot matrix", note: "Visible pixel grid", display: Display::Handheld },
    ShaderOption { path: "handheld/lcd1x_nds", label: "DS LCD", note: "Tuned for DS", display: Display::Handheld },
    ShaderOption { path: "handheld/lcd1x_psp", label: "PSP LCD", note: "Tuned for PSP", display: Display::Handheld },
    ShaderOption { path: "handheld/ds-hybrid-sabr", label: "DS hybrid (SABR)", note: "Smoothed DS upscale", display: Display::Handheld },
    ShaderOption { path: "handheld/lcd-grid-v2", label: "LCD grid v2", note: "Accurate generic grid", display: Display::Handheld },
    ShaderOption { path: "handheld/lcd1x", label: "LCD 1x", note: "Simple generic grid", display: Display::Handheld },
    ShaderOption { path: "handheld/zfast-lcd", label: "LCD — zfast", note: "Cheapest grid", display: Display::Handheld },
    ShaderOption { path: "handheld/dot", label: "Dot", note: "Plain dot matrix", display: Display::Handheld },
];

/// Which display a platform had. Everything not listed is treated as a TV
/// console, which is the safe default: a CRT mask on an unknown system looks
/// wrong, an LCD grid looks broken.
pub fn display_of(platform: &str) -> Display {
    match platform {
        "gb" | "gbc" | "gba" | "gamegear" | "wonderswan" | "wonderswancolor"
        | "neo-geo-pocket" | "nds" | "psp" => Display::Handheld,
        _ => Display::Crt,
    }
}

/// Shipped default per platform.
///
/// Handhelds get their own panel where a specific preset exists, since a
/// Game Boy grid on a PSP looks nothing like the real thing.
pub fn default_for(platform: &str) -> Option<&'static str> {
    Some(match platform {
        "gb" => "handheld/gameboy",
        "gbc" => "handheld/gameboy-color-dot-matrix",
        "gba" => "handheld/agb001",
        "nds" => "handheld/lcd1x_nds",
        "psp" => "handheld/lcd1x_psp",
        "gamegear" | "wonderswan" | "wonderswancolor" | "neo-geo-pocket" => "handheld/lcd1x",
        _ => "crt/crt-guest-advanced",
    })
}

fn shader_root(ra: &RetroArch) -> PathBuf {
    ra.shaders_dir().join("shaders_slang")
}

/// Absolute path of a preset, if this install actually has it.
pub fn resolve(ra: &RetroArch, preset: &str) -> Option<PathBuf> {
    if preset.is_empty() || preset == "none" {
        return None;
    }
    let p = shader_root(ra).join(format!("{preset}.slangp"));
    p.is_file().then_some(p)
}

/// Catalogue entries this install can actually load, for the given display.
pub fn available(ra: &RetroArch, display: Display) -> Vec<&'static ShaderOption> {
    CATALOGUE
        .iter()
        .filter(|o| o.display == display)
        .filter(|o| resolve(ra, o.path).is_some())
        .collect()
}

/// RetroArch config lines enabling `preset`, or disabling shaders entirely.
///
/// Written into the launch overrides rather than the user's config, so this
/// only ever affects games launched from here.
pub fn config_lines(ra: &RetroArch, preset: Option<&str>) -> String {
    match preset.and_then(|p| resolve(ra, p)) {
        Some(path) => format!(
            "\n# Per-platform shader chosen in this app.\n\
             video_shader_enable = \"true\"\nvideo_shader = \"{}\"\n",
            path.display()
        ),
        // Explicitly off: without this a shader set for the previous game
        // would persist into one that should have none.
        None => "\nvideo_shader_enable = \"false\"\n".to_owned(),
    }
}

/// Preset for a platform: the user's choice if set, else the shipped default.
pub fn preset_for(
    overrides: &std::collections::BTreeMap<String, String>,
    platform: &str,
) -> Option<String> {
    if let Some(chosen) = overrides.get(platform) {
        return (chosen != "none" && !chosen.is_empty()).then(|| chosen.clone());
    }
    default_for(platform).map(str::to_owned)
}

/// Label for a preset path, for display.
pub fn label_of(preset: &str) -> &str {
    CATALOGUE
        .iter()
        .find(|o| o.path == preset)
        .map(|o| o.label)
        .unwrap_or(preset)
}

/// Every platform slug we ship a default for, paired with its display kind.
pub fn describe(platform: &str) -> (Display, Option<&'static str>) {
    (display_of(platform), default_for(platform))
}
