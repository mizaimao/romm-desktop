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
    /// Start in this save-state slot instead of at the title screen.
    pub entry_slot: Option<u32>,
    /// Shape the game window like the game rather than filling the screen.
    pub fit_window: bool,
    /// Keep the game window's title bar.
    pub window_decorations: bool,
    /// Where auto-fire lives, if anywhere. See RetroArch::autofire.
    pub autofire: crate::tweaks::AutoFire,
    /// Write a save state when the game exits. Off unless asked for.
    pub save_state_on_exit: bool,
    /// Shots a second, when it is on.
    pub autofire_hz: u32,
    /// Bind players 2-4 like player 1. On by default: the second pad on a desk
    /// is usually the same model as the first, and a pad RetroArch has no
    /// profile for is otherwise a port that does nothing.
    pub mirror_players: bool,
    /// Name the connected controller reports, used to pick the RetroArch
    /// autoconfig profile the gamepad hotkeys are derived from. None means
    /// "whatever this OS's input driver would use".
    pub pad: Option<&'a str>,
    /// RetroAchievements. `None` leaves the user's own settings alone.
    pub achievements: Option<&'a crate::achievements::Settings>,
    /// Systems where the gun replaces a pad, so light gun games can be aimed
    /// with the mouse. Off unless the platform is in here.
    pub lightgun: &'a BTreeMap<String, String>,
    /// The usable area of the display, in the units RetroArch's own window
    /// sizing uses on this platform. `None` leaves the window alone.
    pub screen: Option<crate::retroarch::Screen>,
}

/// A resolved, ready-to-spawn launch.
pub struct Plan {
    /// Load this save-state slot on startup, for "carry on from this one".
    pub entry_slot: Option<u32>,
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
            self.entry_slot,
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
            self.entry_slot,
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
    // absolute path rather than a catalog name keeps config_lines honest:
    // it only ever emits a shader it has verified exists.
    let extra = format!(
        "{}{}{}{}{}{}{}",
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
        ra.prepare_tweaks(req.library_root, req.platform, &core, req.autofire),
        req.achievements.map(crate::achievements::config_lines).unwrap_or_default(),
        crate::lightgun::config_lines(req.platform, gun),
        crate::retroarch::window_lines(
            req.screen,
            // Only when asked for. Someone who wants the window filling the
            // screen and the emulator putting bars in it can have that; the
            // default is a window with no bars in it at all.
            req.fit_window.then(|| crate::aspect::of(req.platform)).flatten(),
            req.window_decorations,
        ),
    );
    let overrides = ra
        .write_overrides_full(
            req.library_root,
            Some(req.user_cfg),
            &extra,
            crate::retroarch::Input {
                pad: req.pad,
                mirror_players: req.mirror_players,
                autofire: req.autofire,
                autofire_hz: req.autofire_hz,
                save_state_on_exit: req.save_state_on_exit,
            },
        )
        .ok();

    // RetroArch's own per-core override, which is applied *after* ours.
    //
    // This is the answer to ten rounds of "rapid fire is still a toggle". The
    // config we write was right; `config/Geolith/Geolith.cfg` on that machine
    // held three lines — turbo mode 2, single button *toggle*, period 6 — and
    // RetroArch loads core overrides after `--appendconfig`, so ours never had
    // a chance. Nothing in the app could see it and nothing said a word.
    //
    // Their file is not ours to edit. Instead: say exactly which keys are
    // being overridden and where, and turn override loading off for this
    // launch so the settings we were asked for are the ones in force.
    if let Some(path) = overrides.as_ref() {
        let clash = ra.override_clash(map.label_for(&core), path);
        if !clash.is_empty() {
            notes.push(format!(
                "RetroArch's own override for {} sets {} — ignoring its override file for \
                 this launch so these settings apply",
                map.label_for(&core).unwrap_or(&core),
                clash.join(", ")
            ));
            // Appended after the fact: the file has to exist to be compared
            // with, and this line only makes sense once something has clashed.
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(path) {
                let _ = writeln!(
                    f,
                    "\n# RetroArch loads config/<core>/<core>.cfg after --appendconfig, and that\n\
                     # file sets things this launch was asked to set. Off for this run only;\n\
                     # the file itself is untouched.\n\
                     auto_overrides_enable = \"false\""
                );
            }
        }
    }

    // Say, in the launch notes, what rapid fire actually ended up in the file.
    //
    // Ten rounds of "it is still a toggle" against a config that reads
    // correctly here, with no way to tell from the outside whether the block
    // was written at all — a pad the autoconfig does not know produces silence
    // that looks exactly like a setting that does not work.
    if req.autofire != crate::tweaks::AutoFire::Off {
        let written = overrides
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .unwrap_or_default();
        notes.push(if written.contains("input_player1_turbo_btn") {
            format!(
                "rapid fire: hold {} on its own for {} shots a second (RetroArch turbo mode 3)",
                match req.autofire {
                    crate::tweaks::AutoFire::RightBumper => "RB",
                    _ => "LB",
                },
                req.autofire_hz
            )
        } else {
            format!(
                "rapid fire: nothing was written — no autoconfig profile for {}",
                req.pad.unwrap_or("this pad")
            )
        });
    }

    Ok(Plan {
        entry_slot: req.entry_slot,
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
/// Whether the gun goes in its port for this launch.
///
/// On unless explicitly switched off, which is the opposite of how this
/// started. Off-by-default was the cautious choice — the gun takes player
/// two's port on the NES, SNES and Mega Drive — but it made the mouse useless
/// in every gun game until you found a per-console tick in Settings and knew
/// what it was for. Nobody found it. A console whose gun games do not work is
/// a worse default than a console whose two-player games need a switch turned
/// off, because the first looks broken and the second is at least visible: the
/// launch notes say the port is a gun, and the app says so on first launch.
fn gun_enabled(map: &BTreeMap<String, String>, platform: &str) -> bool {
    // A console with no gun is never enabled, whatever a stale config says.
    // `config_lines` guards this too, but a function called `gun_enabled`
    // answering "yes" for the Game Boy is a trap for the next caller.
    if !crate::lightgun::supported(platform) {
        return false;
    }
    match map.get(platform).map(|v| v.trim().to_ascii_lowercase()) {
        Some(v) => !matches!(v.as_str(), "off" | "false" | "no" | "0"),
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The gun is in its port unless somebody turned it off. A console with no
    /// gun is unaffected either way.
    #[test]
    fn the_light_gun_is_on_unless_it_is_explicitly_off() {
        let mut map = BTreeMap::new();
        assert!(gun_enabled(&map, "nes"), "unset means on for a console with a gun");
        assert!(!gun_enabled(&map, "gb"), "and stays off where there is no gun");
        for off in ["off", "false", "no", "0", "OFF"] {
            map.insert("nes".to_owned(), off.to_owned());
            assert!(!gun_enabled(&map, "nes"), "{off} must turn it off");
        }
        map.insert("nes".to_owned(), "on".to_owned());
        assert!(gun_enabled(&map, "nes"));
    }

    /// The switch used to be off-by-default and is now on-by-default, so the
    /// only value that has to keep meaning exactly what it did is `off`. A
    /// config written by an older build says `on` for the consoles somebody
    /// chose and nothing for the rest, and both now read as on — which is the
    /// intended change, not a regression.
    #[test]
    fn only_off_survives_from_the_old_switch() {
        let mut map = BTreeMap::new();
        map.insert("nes".to_owned(), "off".to_owned());
        assert!(!gun_enabled(&map, "nes"), "an explicit off is still off");

        // Neither word. Ambiguous, and the safe reading is the default: a gun
        // that works, with a launch note saying the port is a gun.
        map.insert("nes".to_owned(), String::new());
        assert!(gun_enabled(&map, "nes"));

        // A console with no gun ignores the switch whatever it says.
        map.insert("gb".to_owned(), "on".to_owned());
        assert!(!gun_enabled(&map, "gb"));
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
