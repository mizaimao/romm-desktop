//! Everything that has to happen between "play this" and spawning RetroArch.
//!
//! This existed three times — once in the CLI, once in the TUI, once in the
//! Tauri commands — and drifted every time it was touched. Two bugs came
//! directly out of that: per-platform shaders were wired into one path and
//! silently absent from the others, and the `--set-shader` fix later had to be
//! applied three times. The incomplete-playlist guard never made it out of the
//! CLI at all, so the GUI would hand a stub `.m3u` to the emulator and fail
//! somewhere unhelpful.
//!
//! One planner, three thin callers. Adding a step is now one edit.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::coremap::{self, CoreMap};
use crate::retroarch::RetroArch;
use crate::{shaders, tweaks};

/// What the caller knows about the game it wants to run.
pub struct Request<'a> {
    pub rom: &'a Path,
    pub platform: &'a str,
    /// File name as the server knows it, for per-game core overrides.
    pub fs_name: &'a str,
    /// Root of the visible library folder; generated config lands under it.
    pub library_root: &'a Path,
    /// The user's own RetroArch settings file, appended last so it wins.
    pub user_cfg: &'a Path,
    pub shaders_enabled: bool,
    pub shader_overrides: &'a BTreeMap<String, String>,
    pub core_overrides: &'a BTreeMap<String, String>,
    pub core_per_game: &'a BTreeMap<String, String>,
    /// Explicit `--core`, bypassing resolution entirely.
    pub core_override: Option<&'a str>,
    /// Strobe/BFI pass chained on top of the platform's shader, if the user
    /// turned one on. Empty or "none" means no motion pass.
    pub motion_shader: Option<&'a str>,
    /// Display refresh in Hz, as measured by the frontend. Sets how many
    /// subframes the motion pass gets; None falls back to a safe 2.
    pub refresh_hz: Option<f32>,
    /// Name the connected controller reports, used to pick the RetroArch
    /// autoconfig profile the gamepad hotkeys are derived from. None means
    /// "whatever this OS's input driver would use".
    pub pad: Option<&'a str>,
    /// RetroAchievements. `None` leaves the user's own settings alone.
    pub achievements: Option<&'a crate::achievements::Settings>,
    /// Systems where the gun replaces a pad, so light gun games can be aimed
    /// with the mouse. Off unless the platform is in here.
    pub lightgun: &'a BTreeMap<String, String>,
}

/// A resolved, ready-to-spawn launch.
pub struct Plan {
    pub core: String,
    pub core_label: Option<String>,
    pub shader: Option<PathBuf>,
    pub shader_label: Option<String>,
    pub overrides: Option<PathBuf>,
    /// Things worth telling the user, in the order they happened.
    pub notes: Vec<String>,
}

impl Plan {
    /// Spawn and block until the emulator exits.
    pub fn run(&self, ra: &RetroArch, rom: &Path, fullscreen: bool) -> Result<std::process::ExitStatus> {
        ra.launch_full(
            &self.core,
            rom,
            fullscreen,
            self.overrides.as_deref(),
            self.shader.as_deref(),
        )
    }

    /// The same invocation, without running it.
    pub fn command(&self, ra: &RetroArch, rom: &Path, fullscreen: bool) -> Result<std::process::Command> {
        ra.launch_command_full(
            &self.core,
            rom,
            fullscreen,
            self.overrides.as_deref(),
            self.shader.as_deref(),
        )
    }
}

/// Refuse a multi-disc playlist whose discs are not all present.
///
/// RomM indexes `.m3u` files whose disc images it never scanned, which yields
/// a few hundred bytes of text that cannot launch. Catching it here names the
/// missing discs; letting it through fails deep inside the emulator with
/// nothing useful on screen. See PLAN.md §3.
fn check_playlist(rom: &Path) -> Result<()> {
    if !rom.extension().is_some_and(|e| e.eq_ignore_ascii_case("m3u")) {
        return Ok(());
    }
    let dir = rom.parent().unwrap_or(Path::new("."));
    let text = std::fs::read_to_string(rom).unwrap_or_default();
    let missing: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter(|l| !dir.join(l).is_file())
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    let size = std::fs::metadata(rom).map(|m| m.len()).unwrap_or(0);
    bail!(
        "playlist is incomplete ({size} bytes); missing disc(s):\n{}",
        missing.iter().map(|m| format!("  {m}")).collect::<Vec<_>>().join("\n")
    )
}

/// Resolve the core, write the launch config, and report what was done.
pub fn plan(ra: &RetroArch, map: &CoreMap, req: &Request<'_>) -> Result<Plan> {
    let mut notes = Vec::new();

    let core = match req.core_override {
        Some(c) => c.to_owned(),
        None => coremap::resolve_core_for(
            map,
            req.core_overrides,
            req.core_per_game,
            req.platform,
            Some(req.fs_name),
            |c| ra.has_core(c),
        )
        .with_context(|| {
            format!(
                "no installed core for platform {:?}.\n\
                 Install one, set [cores.overrides] in config.toml, or pass --core.",
                req.platform
            )
        })?,
    };

    check_playlist(req.rom)?;

    // Arcade sets need their BIOS in RetroArch's own system directory; copying
    // it is cheap and silent when there is nothing to do.
    if let Some(dir) = req.rom.parent()
        && let Ok(n) = ra.install_bios(dir)
        && n > 0
    {
        notes.push(format!(
            "installed {n} BIOS file(s) into {}",
            ra.system_dir().display()
        ));
    }

    if RetroArch::ensure_user_config(req.user_cfg).unwrap_or(false) {
        notes.push(format!(
            "created {} — put your button map / filters there",
            req.user_cfg.display()
        ));
    }

    let preset = req
        .shaders_enabled
        .then(|| shaders::preset_for(req.shader_overrides, req.platform))
        .flatten();
    // A motion pass has to be chained onto the platform's shader, since
    // RetroArch loads exactly one preset. The generated file replaces the base
    // preset as the thing we point RetroArch at.
    let motion = req
        .motion_shader
        .filter(|m| !m.is_empty() && *m != "none")
        .filter(|_| shaders::display_of(req.platform) == shaders::Display::Crt);
    let chained = motion
        .and_then(|m| shaders::write_chained(ra, req.library_root, preset.as_deref(), m));

    let shader = match &chained {
        Some(p) => Some(p.clone()),
        None => preset.as_deref().and_then(|p| shaders::resolve(ra, p)),
    };
    let shader_label = match (motion, preset.as_deref()) {
        (Some(m), Some(p)) if chained.is_some() => {
            Some(format!("{} + {}", shaders::label_of(p), shaders::label_of(m)))
        }
        (Some(m), None) if chained.is_some() => Some(shaders::label_of(m).to_owned()),
        _ => preset.as_deref().map(|p| shaders::label_of(p).to_owned()),
    };
    if motion.is_some() && chained.is_none() {
        notes.push("motion shader not installed — using the base shader alone".to_owned());
    }

    if let Some(note) = tweaks::describe(req.platform, &core) {
        notes.push(note);
    }
    if let Some(note) = req.achievements.and_then(crate::achievements::describe) {
        notes.push(note);
    }
    let gun = gun_enabled(req.lightgun, req.platform);
    if let Some(note) = crate::lightgun::describe(req.platform, gun) {
        notes.push(note);
    }

    // Point RetroArch at the generated chain when there is one. Passing the
    // absolute path rather than a catalogue name keeps config_lines honest:
    // it only ever emits a shader it has verified exists.
    let extra = format!(
        "{}{}{}{}{}{}",
        match &chained {
            Some(p) => format!(
                "\n# Base shader with a motion pass chained on.\n\
                 video_shader_enable = \"true\"\nvideo_shader = \"{}\"\n\
                 video_shader_subframes = \"{}\"\n",
                p.display(),
                shaders::subframes_for(req.refresh_hz)
            ),
            None => String::new(),
        },
        match &chained {
            Some(_) => String::new(),
            None => shaders::config_lines(ra, preset.as_deref()),
        },
        ra.system_dir_line(),
        ra.prepare_tweaks(req.library_root, req.platform, &core),
        req.achievements.map(crate::achievements::config_lines).unwrap_or_default(),
        crate::lightgun::config_lines(req.platform, gun),
    );
    let overrides = ra
        .write_overrides_full(req.library_root, Some(req.user_cfg), &extra, req.pad)
        .ok();

    Ok(Plan {
        core_label: map.label_for(&core).map(str::to_owned),
        core,
        shader,
        shader_label,
        overrides,
        notes,
    })
}

/// Whether the light gun switch is on for a platform.
///
/// Stored as text rather than a bool because it rides the same per-platform
/// config table as the core and shader choices, which are strings. Anything
/// that is not an explicit yes counts as off — a half-written value should
/// leave port 2 as a pad, not turn it into a gun.
fn gun_enabled(map: &BTreeMap<String, String>, platform: &str) -> bool {
    map.get(platform)
        .map(|v| matches!(v.trim(), "on" | "true" | "yes" | "1"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_light_gun_switch_is_off_unless_it_is_explicitly_on() {
        let mut map = BTreeMap::new();
        assert!(!gun_enabled(&map, "nes"));
        map.insert("nes".to_owned(), "off".to_owned());
        assert!(!gun_enabled(&map, "nes"));
        // A value written by an older build, or by hand, that is neither.
        map.insert("nes".to_owned(), String::new());
        assert!(!gun_enabled(&map, "nes"));
        map.insert("nes".to_owned(), "on".to_owned());
        assert!(gun_enabled(&map, "nes"));
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("romm-launch-test-{name}"));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Anything that is not a playlist is none of this check's business.
    #[test]
    fn a_plain_rom_is_never_treated_as_a_playlist() {
        let dir = scratch("not-m3u");
        let rom = dir.join("Sonic.md");
        std::fs::write(&rom, b"whatever").unwrap();
        assert!(check_playlist(&rom).is_ok());
        // Not even one that does not exist — the core's own error is better
        // than a made-up one about discs.
        assert!(check_playlist(&dir.join("Missing.md")).is_ok());
    }

    /// The extension check is case-insensitive; RomM's library carries both.
    #[test]
    fn a_complete_playlist_launches() {
        let dir = scratch("complete");
        std::fs::write(dir.join("d1.chd"), b"a").unwrap();
        std::fs::write(dir.join("d2.chd"), b"b").unwrap();
        for name in ["Game.m3u", "Game.M3U"] {
            let m3u = dir.join(name);
            std::fs::write(&m3u, "d1.chd\nd2.chd\n").unwrap();
            assert!(check_playlist(&m3u).is_ok(), "{name} should pass");
        }
    }

    /// The bug this exists for: RomM indexes .m3u files whose disc images it
    /// never scanned, producing a few hundred bytes of text that cannot launch.
    /// Caught here it names the discs; passed through it fails deep inside the
    /// emulator with nothing useful on screen.
    #[test]
    fn an_incomplete_playlist_names_the_discs_that_are_missing() {
        let dir = scratch("incomplete");
        std::fs::write(dir.join("d1.chd"), b"a").unwrap();
        let m3u = dir.join("Game.m3u");
        std::fs::write(&m3u, "d1.chd\nd2.chd\nd3.chd\n").unwrap();

        let err = check_playlist(&m3u).expect_err("two discs are absent").to_string();
        assert!(err.contains("d2.chd"), "must name the missing disc: {err}");
        assert!(err.contains("d3.chd"), "and all of them: {err}");
        assert!(!err.contains("d1.chd"), "not the one that is present: {err}");
    }

    /// Comments and blank lines are structure, not filenames. Treating them as
    /// discs would fail every playlist that has either.
    #[test]
    fn comments_and_blank_lines_are_not_discs() {
        let dir = scratch("comments");
        std::fs::write(dir.join("d1.chd"), b"a").unwrap();
        let m3u = dir.join("Game.m3u");
        std::fs::write(&m3u, "# Disc listing\n\n  d1.chd  \n\n#d2.chd\n").unwrap();
        assert!(check_playlist(&m3u).is_ok(), "only d1.chd is a real entry");
    }
}
