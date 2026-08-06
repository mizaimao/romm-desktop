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
    /// Name the connected controller reports, used to pick the RetroArch
    /// autoconfig profile the gamepad hotkeys are derived from. None means
    /// "whatever this OS's input driver would use".
    pub pad: Option<&'a str>,
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
    let shader = preset.as_deref().and_then(|p| shaders::resolve(ra, p));
    let shader_label = preset.as_deref().map(|p| shaders::label_of(p).to_owned());

    if let Some(note) = tweaks::describe(req.platform, &core) {
        notes.push(note);
    }

    let extra = format!(
        "{}{}{}",
        shaders::config_lines(ra, preset.as_deref()),
        ra.system_dir_line(),
        ra.prepare_tweaks(req.library_root, req.platform, &core)
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
