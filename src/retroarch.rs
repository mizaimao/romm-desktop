//! Locating and launching a RetroArch install.
//!
//! Deliberately does not assume `/Applications/RetroArch.app`: this machine's
//! install lives elsewhere and runs in portable mode, which is the layout we
//! target. See PLAN.md §6 for how portable mode resolves directories.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

/// A located RetroArch install.
#[derive(Debug)]
pub struct RetroArch {
    /// Directory containing `RetroArch.app`. In portable mode this is also the
    /// root for `cores/`, `saves/`, `states/`, `system/`, `config/`.
    pub root: PathBuf,
    pub binary: PathBuf,
    /// True when `portable.txt` sits beside the bundle, meaning RetroArch keeps
    /// everything under `root` instead of `~/Documents` + `~/Library`.
    pub portable: bool,
}

/// Roots checked when config.toml does not name one.
const CANDIDATE_ROOTS: &[&str] = &[
    "/Applications",
    "~/Applications",
    "~/Data/Games/Emulators/RetroArch",
];

fn expand_tilde(p: &str) -> PathBuf {
    match p.strip_prefix("~/") {
        Some(rest) => match std::env::var_os("HOME") {
            Some(home) => PathBuf::from(home).join(rest),
            None => PathBuf::from(p),
        },
        None => PathBuf::from(p),
    }
}

impl RetroArch {
    /// Locate an install. `configured` wins; otherwise probe known roots.
    pub fn locate(configured: Option<&str>) -> Result<Self> {
        let mut tried: Vec<PathBuf> = Vec::new();

        let candidates: Vec<PathBuf> = match configured {
            Some(c) => vec![expand_tilde(c)],
            None => CANDIDATE_ROOTS.iter().map(|c| expand_tilde(c)).collect(),
        };

        for root in candidates {
            // Accept either the directory holding RetroArch.app, or the bundle.
            let bundle = if root.extension().is_some_and(|e| e == "app") {
                root.clone()
            } else {
                root.join("RetroArch.app")
            };
            tried.push(bundle.clone());
            let binary = bundle.join("Contents/MacOS/RetroArch");
            if binary.is_file() {
                let root = bundle
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| root.clone());
                let portable = root.join("portable.txt").is_file();
                return Ok(Self {
                    root,
                    binary,
                    portable,
                });
            }
        }

        bail!(
            "could not find RetroArch. Tried:\n{}\nSet [retroarch] root in config.toml.",
            tried
                .iter()
                .map(|p| format!("  {}", p.display()))
                .collect::<Vec<_>>()
                .join("\n")
        )
    }

    /// Directory holding `*_libretro.dylib`.
    ///
    /// Only correct for builds with `HAVE_UPDATE_CORES`, which is what the
    /// official download is; App Store builds keep cores inside the bundle.
    /// Verified against this machine's 1.20.0 install.
    pub fn cores_dir(&self) -> PathBuf {
        self.root.join("cores")
    }

    pub fn core_path(&self, core: &str) -> PathBuf {
        self.cores_dir().join(format!("{core}_libretro.dylib"))
    }

    pub fn has_core(&self, core: &str) -> bool {
        self.core_path(core).is_file()
    }

    /// Core stems currently installed.
    pub fn installed_cores(&self) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(self.cores_dir()) else {
            return Vec::new();
        };
        let mut out: Vec<String> = entries
            .flatten()
            .filter_map(|e| {
                e.file_name()
                    .to_str()
                    .and_then(|n| n.strip_suffix("_libretro.dylib"))
                    .map(str::to_owned)
            })
            .collect();
        out.sort();
        out
    }

    /// Build the launch command. Does not spawn.
    ///
    /// The existing portable `config/retroarch.cfg` is used deliberately — we
    /// do not pass `-c` yet, so the spike tests launching against a known-good
    /// setup rather than a config we invented.
    ///
    /// `fullscreen` adds `-f`. It defaults to off at the call site: RetroArch's
    /// own config decides otherwise, and taking over the display uninvited is
    /// obnoxious.
    pub fn launch_command(&self, core: &str, rom: &Path, fullscreen: bool) -> Result<Command> {
        let core_path = self.core_path(core);
        if !core_path.is_file() {
            bail!(
                "core not installed: {}\n  expected at {}",
                core,
                core_path.display()
            );
        }
        if !rom.is_file() {
            bail!("ROM not found: {}", rom.display());
        }
        let mut cmd = Command::new(&self.binary);
        cmd.arg("-L").arg(&core_path).arg(rom);
        if fullscreen {
            cmd.arg("-f");
        }
        Ok(cmd)
    }

    /// Spawn and block until the emulator exits.
    pub fn launch(&self, core: &str, rom: &Path, fullscreen: bool) -> Result<std::process::ExitStatus> {
        let mut cmd = self.launch_command(core, rom, fullscreen)?;
        let status = cmd
            .status()
            .with_context(|| format!("spawning {}", self.binary.display()))?;
        Ok(status)
    }
}

/// Render a command the way a shell would accept it.
pub fn render(cmd: &Command) -> String {
    let quote = |s: &str| {
        if s.contains([' ', '\'', '"', '(', ')', '!']) {
            format!("{:?}", s)
        } else {
            s.to_owned()
        }
    };
    let mut parts = vec![quote(&cmd.get_program().to_string_lossy())];
    parts.extend(cmd.get_args().map(|a| quote(&a.to_string_lossy())));
    parts.join(" ")
}
