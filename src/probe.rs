//! Deterministic "does this core actually run this game?" test.
//!
//! The previous attempt at this launched a game, waited a few seconds, killed
//! it, and called it a pass if nothing had crashed. That gave contradictory
//! results on identical runs — the same game passed three times and failed a
//! fourth — and a default was once changed on the strength of it. None of that
//! evidence was worth anything.
//!
//! This is the honest version, built on three RetroArch features:
//!
//! * `--max-frames=N` runs exactly N frames and exits. No timers, no killing.
//! * `--log-file` + `--verbose` record what the core actually did.
//! **This is not headless.** `video_driver = "null"` silences rendering, but on
//! macOS RetroArch's Cocoa frontend creates its window before the video driver
//! is chosen, so a window still appears for every single probe. There is no way
//! around that here. Probing N games against M cores opens N*M windows, so this
//! is gated behind an explicit flag and must never be run unasked.
//!
//! **The exit code is 0 whether or not the content loaded**, which is the trap
//! the old method fell into. The verdict comes from the log alone.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::retroarch::RetroArch;

/// What a single (game, core) probe concluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The core loaded the content and ran frames.
    Ran,
    /// The core refused the content — wrong system, missing romset files,
    /// version mismatch.
    RefusedContent,
    /// The core itself would not load (missing, wrong architecture).
    CoreFailed,
    /// RetroArch never reached a verdict; treat as unknown, not as a pass.
    Inconclusive,
}

impl Verdict {
    pub fn label(self) -> &'static str {
        match self {
            Verdict::Ran => "ran",
            Verdict::RefusedContent => "refused content",
            Verdict::CoreFailed => "core failed to load",
            Verdict::Inconclusive => "inconclusive",
        }
    }

    pub fn ok(self) -> bool {
        self == Verdict::Ran
    }
}

#[derive(Debug, Clone)]
pub struct Probe {
    pub core: String,
    pub verdict: Verdict,
    /// The line the verdict was read from, for when a result looks wrong.
    pub evidence: String,
    pub seconds: f64,
}

/// Config silencing audio and rendering.
///
/// Does NOT prevent the window on macOS — see the module docs. Written per run
/// rather than kept around: it must never be mistaken for the user's own
/// RetroArch config.
const HEADLESS: &str = "\
# Written by `romm-desktop probe`. Nothing here should reach a real session.
#
# config_save_on_exit MUST stay first and MUST stay false. RetroArch defaults it
# to true, and an --appendconfig value is written back into the user's
# retroarch.cfg on exit — an earlier version of this file omitted it and
# permanently set the user's video, audio and menu drivers to \"null\", which
# silently broke every game until someone noticed.
config_save_on_exit = \"false\"
video_driver = \"null\"
audio_driver = \"null\"
menu_driver = \"null\"
video_fullscreen = \"false\"
# A probe must not inherit or write save state, or it would pollute real saves.
savestate_auto_load = \"false\"
savestate_auto_save = \"false\"
";

/// Run one game under one core and report what happened.
///
/// `frames` at 60 fps: 180 is three seconds, enough for a core to load a
/// romset and start emulating, short enough to probe hundreds of games.
pub fn probe_one(ra: &RetroArch, rom: &Path, core: &str, frames: u32, scratch: &Path) -> Result<Probe> {
    std::fs::create_dir_all(scratch)
        .with_context(|| format!("creating {}", scratch.display()))?;
    let cfg = scratch.join("headless.cfg");
    std::fs::write(&cfg, HEADLESS)?;
    let log = scratch.join("probe.log");
    let _ = std::fs::remove_file(&log);

    let started = std::time::Instant::now();
    let out = Command::new(&ra.binary)
        .arg(format!("--appendconfig={}", cfg.display()))
        .arg(format!("--max-frames={frames}"))
        .arg("--verbose")
        .arg(format!("--log-file={}", log.display()))
        .arg("-L")
        .arg(core)
        .arg(rom)
        .output()
        .with_context(|| format!("running RetroArch for {core}"))?;
    let seconds = started.elapsed().as_secs_f64();

    // Deliberately ignoring out.status: RetroArch exits 0 even when the core
    // refused the content. Only the log distinguishes the two.
    let text = std::fs::read_to_string(&log).unwrap_or_else(|_| {
        String::from_utf8_lossy(&out.stderr).into_owned()
    });
    let (verdict, evidence) = read_verdict(&text);
    Ok(Probe { core: core.to_owned(), verdict, evidence, seconds })
}

/// Read a verdict out of a RetroArch log.
///
/// Order matters: a failure line is conclusive even though the log also
/// contains the "Content ran for" line that a success would produce.
pub fn read_verdict(log: &str) -> (Verdict, String) {
    let mut ran_line = None;
    for line in log.lines() {
        if line.contains("[Content]: Failed to load content")
            || line.contains("Failed to load content")
        {
            return (Verdict::RefusedContent, line.trim().to_owned());
        }
        if line.contains("Failed to open libretro core")
            || line.contains("Failed to load libretro core")
            || line.contains("[Core]: Failed to load")
        {
            return (Verdict::CoreFailed, line.trim().to_owned());
        }
        if line.contains("Content ran for a total of") {
            ran_line = Some(line.trim().to_owned());
        }
    }

    match ran_line {
        // "00 hours, 00 minutes, 00 seconds" means it never actually emulated
        // anything, which is a refusal the core did not log explicitly.
        Some(l) if l.contains("00 hours, 00 minutes, 00 seconds") => {
            (Verdict::RefusedContent, l)
        }
        Some(l) => (Verdict::Ran, l),
        None => (Verdict::Inconclusive, "no verdict line in the log".to_owned()),
    }
}

/// Try every candidate core against one game, in order.
pub fn probe_cores(
    ra: &RetroArch,
    rom: &Path,
    cores: &[String],
    frames: u32,
    scratch: &Path,
) -> Result<Vec<Probe>> {
    if !rom.is_file() && !rom.is_dir() {
        bail!("not downloaded: {}", rom.display());
    }
    let mut out = Vec::new();
    for core in cores {
        if !ra.has_core(core) {
            out.push(Probe {
                core: core.clone(),
                verdict: Verdict::CoreFailed,
                evidence: "core not installed".to_owned(),
                seconds: 0.0,
            });
            continue;
        }
        out.push(probe_one(ra, rom, core, frames, scratch)?);
    }
    Ok(out)
}

/// Scratch directory for probe artefacts, inside the visible library folder
/// rather than a hidden system temp dir.
pub fn scratch_dir(library_root: &str) -> PathBuf {
    PathBuf::from(library_root).join("probe")
}
