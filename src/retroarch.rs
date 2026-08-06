//! Locating and launching a RetroArch install.
//!
//! Deliberately does not assume `/Applications/RetroArch.app`: this machine's
//! install lives elsewhere and runs in portable mode, which is the layout we
//! target. See PLAN.md §6 for how portable mode resolves directories.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::util::expand_tilde;

/// Seed for the user's own RetroArch settings.
///
/// Everything is commented out: this file is appended last at launch, so an
/// uncommented line takes effect immediately and silently changing someone's
/// controls would be worse than doing nothing.
const USER_CONFIG_TEMPLATE: &str = r#"# Your RetroArch settings, applied every time this app launches a game.
#
# Same format as retroarch.cfg: key = "value". Appended AFTER our defaults, so
# anything here wins. Your own retroarch.cfg is never modified.
#
# Find the exact key for a setting by changing it once in RetroArch's menu and
# diffing its retroarch.cfg, or see docs.libretro.com.

# ---- Video filters / shaders ----
# video_shader_enable = "true"
# video_shader = "~/Data/Games/Emulators/RetroArch/shaders/shaders_glsl/crt/crt-geom.glslp"
# video_smooth = "false"
# video_scale_integer = "true"

# ---- Controls ----
# Player 1, RetroPad button -> your device's button index.
# input_player1_a_btn = "1"
# input_player1_b_btn = "0"
# input_player1_x_btn = "3"
# input_player1_y_btn = "2"
# input_player1_l_btn = "4"
# input_player1_r_btn = "5"
# input_player1_start_btn = "9"
# input_player1_select_btn = "8"
# input_player1_up_btn = "h0up"
# input_player1_down_btn = "h0down"
# input_player1_left_btn = "h0left"
# input_player1_right_btn = "h0right"

# ---- Hotkeys ----
# input_enable_hotkey_btn = "8"
# input_exit_emulator_btn = "9"
# input_menu_toggle_btn = "2"

# ---- Anything else ----
# audio_latency = "64"
# fastforward_ratio = "3.0"
"#;

/// A located RetroArch install.
#[derive(Debug)]
pub struct RetroArch {
    /// Set when BIOS should be read from somewhere other than
    /// `<root>/system`; see [`Self::with_system_dir`].
    pub system_override: Option<PathBuf>,
    /// Directory containing `RetroArch.app`. In portable mode this is also the
    /// root for `cores/`, `saves/`, `states/`, `system/`, `config/`.
    pub root: PathBuf,
    pub binary: PathBuf,
    /// True when `portable.txt` sits beside the bundle, meaning RetroArch keeps
    /// everything under `root` instead of `~/Documents` + `~/Library`.
    pub portable: bool,
}

/// Roots checked when config.toml does not name one.
/// Where RetroArch usually lives, per OS, in probe order.
///
/// The shape of an install differs enough between platforms that one list will
/// not do: macOS ships an `.app` bundle, Windows a directory with
/// `retroarch.exe`, and Linux normally installs to a system prefix with cores
/// under `~/.config` or `/usr/lib`.
const CANDIDATE_ROOTS: &[&str] = &[
    #[cfg(target_os = "macos")]
    "/Applications",
    #[cfg(target_os = "macos")]
    "~/Applications",
    #[cfg(target_os = "macos")]
    "~/Data/Games/Emulators/RetroArch",
    #[cfg(target_os = "windows")]
    "C:/RetroArch-Win64",
    #[cfg(target_os = "windows")]
    "C:/Program Files/RetroArch",
    #[cfg(target_os = "windows")]
    "C:/Program Files (x86)/RetroArch",
    #[cfg(target_os = "windows")]
    "~/RetroArch",
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    "/usr",
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    "/usr/local",
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    "/app",
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    "~/.local",
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    "~/RetroArch",
];

/// Executable name for the host, and where it sits under the install root.
///
/// Returns `(subpath of the binary, subpath of the "root" we should record)`.
/// macOS hides the binary inside the bundle and treats the *containing*
/// directory as the root, because that is where `portable.txt` and `cores/`
/// live. The other two put the executable in the root itself.
/// First path that exists, preferring `primary`; falls back to the first
/// `primary` entry so callers always get a usable path to report.
fn first_existing(primary: &[PathBuf], extra: &[PathBuf]) -> PathBuf {
    primary
        .iter()
        .chain(extra.iter())
        .find(|p| p.is_dir())
        .cloned()
        .unwrap_or_else(|| primary[0].clone())
}

fn binary_candidates(root: &Path) -> Vec<(PathBuf, PathBuf)> {
    #[cfg(target_os = "macos")]
    {
        let bundle = if root.extension().is_some_and(|e| e == "app") {
            root.to_path_buf()
        } else {
            root.join("RetroArch.app")
        };
        let recorded = bundle.parent().map(Path::to_path_buf).unwrap_or_else(|| root.to_path_buf());
        vec![(bundle.join("Contents/MacOS/RetroArch"), recorded)]
    }
    #[cfg(target_os = "windows")]
    {
        vec![(root.join("retroarch.exe"), root.to_path_buf())]
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        // A distro install puts the binary in <prefix>/bin but keeps cores and
        // config elsewhere, so both layouts are worth trying.
        vec![
            (root.join("bin").join("retroarch"), root.to_path_buf()),
            (root.join("retroarch"), root.to_path_buf()),
        ]
    }
}

impl RetroArch {
    /// Locate an install. `configured` wins; otherwise probe known roots.
    pub fn locate(configured: Option<&str>) -> Result<Self> {
        Self::locate_in(&configured.map(str::to_owned).into_iter().collect::<Vec<_>>())
    }

    /// Try each path in order and take the first that holds RetroArch.
    ///
    /// An empty list means "probe the usual places". Ordering is the point:
    /// it lets a portable build take precedence over a system one without
    /// uninstalling anything.
    pub fn locate_in(paths: &[String]) -> Result<Self> {
        let mut tried: Vec<PathBuf> = Vec::new();

        let candidates: Vec<PathBuf> = if paths.is_empty() {
            CANDIDATE_ROOTS.iter().map(|c| expand_tilde(c)).collect()
        } else {
            paths.iter().map(|p| expand_tilde(p)).collect()
        };

        for root in candidates {
            for (binary, recorded) in binary_candidates(&root) {
                tried.push(binary.clone());
                if !binary.is_file() {
                    continue;
                }
                // portable.txt is a macOS and Windows mechanism; on Linux
                // RetroArch always follows XDG and ignores the marker
                // entirely (PLAN.md §6).
                let portable = cfg!(not(target_os = "linux"))
                    && recorded.join("portable.txt").is_file();
                return Ok(Self {
                    root: recorded,
                    binary,
                    portable,
                    system_override: None,
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

    /// Directory holding `*_libretro.<dylib|dll|so>`.
    ///
    /// Only correct for builds with `HAVE_UPDATE_CORES`, which is what the
    /// official download is; App Store builds keep cores inside the bundle.
    /// Verified against this machine's 1.20.0 install.
    pub fn cores_dir(&self) -> PathBuf {
        first_existing(
            &[self.data_dir().join("cores")],
            // Distro packages drop cores in a system library directory instead
            // of the user's data folder; Flatpak uses its own sandbox path.
            &[
                PathBuf::from("/usr/lib/x86_64-linux-gnu/libretro"),
                PathBuf::from("/usr/lib/libretro"),
                PathBuf::from("/usr/local/lib/libretro"),
            ],
        )
    }

    /// RetroArch's user-data root: where `retroarch.cfg`, `cores/`, `system/`
    /// and `config/` all hang off.
    ///
    /// In portable mode that is the install directory. Otherwise it is the
    /// platform's own location, which is the part that differs: macOS uses
    /// Application Support, Windows uses APPDATA, and Linux follows XDG —
    /// where `portable.txt` is ignored outright, so the marker is never even
    /// consulted there.
    pub fn data_dir(&self) -> PathBuf {
        if self.portable {
            return self.root.clone();
        }
        #[cfg(target_os = "macos")]
        {
            crate::util::expand_tilde("~/Library/Application Support/RetroArch")
        }
        #[cfg(target_os = "windows")]
        {
            std::env::var_os("APPDATA")
                .map(|a| PathBuf::from(a).join("RetroArch"))
                .unwrap_or_else(|| self.root.clone())
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| crate::util::expand_tilde("~/.config"))
                .join("retroarch")
        }
    }

    /// Where per-core settings live: `<config>/<Core>/<Core>.opt`.
    pub fn config_dir(&self) -> PathBuf {
        self.data_dir().join("config")
    }

    /// Root of the shader trees (`shaders_slang`, `shaders_glsl`).
    pub fn shaders_dir(&self) -> PathBuf {
        first_existing(
            &[self.data_dir().join("shaders"), self.root.join("shaders")],
            &[
                PathBuf::from("/usr/share/libretro/shaders"),
                PathBuf::from("/usr/local/share/libretro/shaders"),
            ],
        )
    }

    pub fn core_path(&self, core: &str) -> PathBuf {
        self.cores_dir()
            .join(format!("{core}_libretro.{}", crate::cores::lib_extension()))
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
                    .and_then(|n| {
                        n.strip_suffix(&format!("_libretro.{}", crate::cores::lib_extension()))
                    })
                    .map(str::to_owned)
            })
            .collect();
        out.sort();
        out
    }

    /// Settings we force at launch, written to a file passed via
    /// `--appendconfig`.
    ///
    /// A RetroArch tuned for a handheld makes poor assumptions on a desktop
    /// launched from another app. These are overlaid for the session only —
    /// the user's own `retroarch.cfg` is never modified.
    const OVERRIDES: &str = "\
# Generated by romm-desktop. Applied per launch via --appendconfig.
# Your own retroarch.cfg is not modified.

# Without this RetroArch starts paused whenever its window is not focused,
# which is always true when launched from another application: the game never
# starts and a pause icon sits in the corner.
pause_nonactive = \"false\"

# Bezel/border overlays are for handhelds; on a desktop they show up as
# decorative bars around a small screen.
input_overlay_enable = \"false\"

# A saved custom viewport (e.g. 1920x1440) distorts a 160x144 handheld game in
# a resizable window. Let the core state its own aspect.
aspect_ratio_index = \"21\"
video_aspect_ratio_auto = \"true\"

# Belt and braces: never let a launch from here rewrite the user's config.
config_save_on_exit = \"false\"

# ---- Controller hotkeys ----
# RetroArch ships keyboard hotkeys (F1 menu, Escape exit) but binds none for a
# gamepad, so a handheld user has no way out of a game without a keyboard.
# These give every install launched from here the same combinations.
#
# Select acts as the modifier, so none of these fire during normal play: a
# hotkey needs Select held as well. Buttons are given by standard-mapping
# index, which is what RetroArch reports for Xbox, DualSense, Switch Pro and
# 8BitDo pads alike.
input_enable_hotkey_btn = \"8\"          # Select / Share — modifier
input_menu_toggle_btn = \"9\"            # + Start  -> RetroArch menu
input_exit_emulator_btn = \"1\"          # + B/Circle -> quit
input_save_state_btn = \"3\"             # + Y/Triangle
input_load_state_btn = \"0\"             # + A/Cross
input_screenshot_btn = \"2\"             # + X/Square
input_pause_toggle_btn = \"10\"          # + L3
input_hold_fast_forward_btn = \"5\"      # + R1
input_rewind_btn = \"4\"                 # + L1
";

    /// Windows-only additions, appended to `OVERRIDES` there.
    #[cfg(target_os = "windows")]
    const OVERRIDES_OS: &str = "\

# ---- Windows ----
# The GL driver flickers while a window is being resized, badly enough to look
# broken. D3D11 does not, and is the driver the Windows build is tuned for.
# Override this in your own settings file if your GPU prefers something else.
video_driver = \"d3d11\"

# Resizing tears without this once the driver is switched.
video_vsync = \"true\"
";

    #[cfg(not(target_os = "windows"))]
    const OVERRIDES_OS: &str = "";
    /// As above, appending the user's own settings last so they win.
    ///
    /// RetroArch applies `--appendconfig` entries in order, later overriding
    /// earlier, so anything in `user_config` beats our defaults. That is how a
    /// pinned button map or video filter survives without editing RetroArch's
    /// own config or opening its menu.
    pub fn write_overrides_with(
        &self,
        dir: &Path,
        user_config: Option<&Path>,
    ) -> Result<PathBuf> {
        self.write_overrides_full(dir, user_config, "")
    }

    /// As above, with `extra` (per-platform shader settings) inserted before
    /// the user's file — so the user can still override even those.
    pub fn write_overrides_full(
        &self,
        dir: &Path,
        user_config: Option<&Path>,
        extra: &str,
    ) -> Result<PathBuf> {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("creating {}", dir.display()))?;
        let mut body = Self::OVERRIDES.to_owned();
        body.push_str(Self::OVERRIDES_OS);
        body.push_str(extra);

        if let Some(user) = user_config {
            match std::fs::read_to_string(user) {
                Ok(extra) => {
                    body.push_str(&format!(
                        "\n# ---- from {} (yours; overrides everything above) ----\n",
                        user.display()
                    ));
                    body.push_str(&extra);
                    if !extra.ends_with('\n') {
                        body.push('\n');
                    }
                }
                Err(e) => eprintln!("warning: could not read {}: {e}", user.display()),
            }
        }

        let path = dir.join("retroarch-overrides.cfg");
        std::fs::write(&path, &body)
            .with_context(|| format!("writing {}", path.display()))?;
        Ok(path)
    }

    /// Write a starter user-settings file if none exists yet.
    ///
    /// Seeded with commented examples rather than active values: silently
    /// changing someone's controls is worse than leaving them alone.
    pub fn ensure_user_config(path: &Path) -> Result<bool> {
        if path.exists() {
            return Ok(false);
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).ok();
        }
        std::fs::write(path, USER_CONFIG_TEMPLATE)
            .with_context(|| format!("writing {}", path.display()))?;
        Ok(true)
    }

    /// RetroArch's own system directory, where cores look for BIOS files.
    pub fn system_dir(&self) -> PathBuf {
        self.system_override.clone().unwrap_or_else(|| self.root.join("system"))
    }

    /// Point BIOS lookups at a folder we control instead of RetroArch's own.
    ///
    /// The canonical BIOS set is kept on the server and synced into the visible
    /// library folder, so a second machine gets an identical set by copying one
    /// directory. RetroArch's own `system/` is left alone.
    ///
    /// An earlier attempt at this set `system_directory` to the *ROM's* folder
    /// and broke every arcade launch, because the MAME BIOS romsets were not
    /// there. The folder handed to this must be a superset — arcade BIOS
    /// included — or the same breakage returns.
    pub fn with_system_dir(mut self, dir: Option<PathBuf>) -> Self {
        // Absolute, always: RetroArch resolves a relative `system_directory`
        // against its own working directory rather than ours.
        self.system_override = dir
            .filter(|d| d.is_dir())
            .map(|d| d.canonicalize().unwrap_or(d));
        self
    }

    /// Copy known BIOS sets sitting beside a ROM into RetroArch's system
    /// directory.
    ///
    /// MAME-family cores look for BIOS only in the system directory, so a
    /// `neogeo.zip` downloaded alongside the games is invisible to them —
    /// "Neo Geo BIOS required" even though the file is right there. Copying is
    /// cheap (a couple of MB) and leaves the download layout untouched.
    pub fn install_bios(&self, rom_dir: &Path) -> Result<usize> {
        const BIOS: &[&str] = &["neogeo.zip", "neocdz.zip", "pgm.zip", "decocass.zip"];
        let dest = self.system_dir();
        std::fs::create_dir_all(&dest).ok();
        let mut n = 0;
        for name in BIOS {
            let src = rom_dir.join(name);
            let dst = dest.join(name);
            if src.is_file() && !dst.is_file() && std::fs::copy(&src, &dst).is_ok() {
                n += 1;
            }
        }
        Ok(n)
    }
    /// As [`Self::launch_command`], additionally appending an override config.
    pub fn launch_command_with(
        &self,
        core: &str,
        rom: &Path,
        fullscreen: bool,
        overrides: Option<&Path>,
    ) -> Result<Command> {
        self.launch_command_full(core, rom, fullscreen, overrides, None)
    }

    /// The full form, including an explicit shader preset.
    ///
    /// `--set-shader` rather than the `video_shader` config key. Writing the
    /// key into an `--appendconfig` file looked right and did nothing: for
    /// RetroArch that key is remembered *state*, and what actually loads a
    /// preset with content is this flag, which its own help describes as
    /// "loaded each time content is loaded, effectively overrides automatic
    /// shader presets". Passing an empty string disables shaders explicitly,
    /// so a preset set for the previous game cannot leak into this one.
    pub fn launch_command_full(
        &self,
        core: &str,
        rom: &Path,
        fullscreen: bool,
        overrides: Option<&Path>,
        shader: Option<&Path>,
    ) -> Result<Command> {
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
        if let Some(cfg) = overrides {
            cmd.arg("--appendconfig").arg(cfg);
        }
        match shader {
            Some(p) => {
                cmd.arg(format!("--set-shader={}", p.display()));
            }
            None => {
                cmd.arg("--set-shader=");
            }
        }
        if fullscreen {
            cmd.arg("-f");
        }
        Ok(cmd)
    }

    /// Write project-local core options and remaps for this platform, and
    /// return the config lines that point RetroArch at them.
    ///
    /// Returns an empty string when the platform needs neither, so the
    /// redirect stays off and every other core keeps using the user's own
    /// per-core settings untouched.
    /// Config line pointing BIOS lookups at our folder, when one is set.
    pub fn system_dir_line(&self) -> String {
        match &self.system_override {
            Some(d) => format!(
                "\n# BIOS come from the library folder, synced from the server, so a\n                 # second machine gets an identical set by copying one directory.\n                 system_directory = \"{}\"\n",
                d.display()
            ),
            None => String::new(),
        }
    }

    pub fn prepare_tweaks(&self, library_root: &Path, platform: &str, core: &str) -> String {
        let opts = crate::tweaks::core_options(platform, core);
        let remap = crate::tweaks::remap(platform, core);
        if opts.is_empty() && remap.is_empty() {
            return String::new();
        }

        let Some(label) = crate::tweaks::core_dir_name(core) else {
            return String::new();
        };
        let dir = library_root.join("retroarch");
        if std::fs::create_dir_all(&dir).is_err() {
            return String::new();
        }
        // Absolute: RetroArch resolves a relative path against its own working
        // directory, not ours, so "./library/..." would silently miss.
        let dir = dir.canonicalize().unwrap_or(dir);

        // Seed from the user's own options for this core so their choices —
        // palette, aspect, sound quality — survive the redirect. Only the keys
        // we care about are then overwritten.
        let user_opt = self
            .config_dir()
            .join(label)
            .join(format!("{label}.opt"));
        let mut lines: Vec<String> = std::fs::read_to_string(&user_opt)
            .map(|s| s.lines().map(str::to_owned).filter(|l| !l.trim().is_empty()).collect())
            .unwrap_or_default();
        for (k, v) in opts {
            lines.retain(|l| l.split('=').next().is_none_or(|key| key.trim() != *k));
            lines.push(format!("{k} = \"{v}\""));
        }
        lines.sort();
        let opts_path = dir.join("core-options.cfg");
        if std::fs::write(&opts_path, lines.join("\n") + "\n").is_err() {
            return String::new();
        }

        let remaps_dir = dir.join("remaps");
        if !remap.is_empty() {
            let core_dir = remaps_dir.join(label);
            if std::fs::create_dir_all(&core_dir).is_ok() {
                let _ = std::fs::write(
                    core_dir.join(format!("{label}.rmp")),
                    remap.join("\n") + "\n",
                );
            }
        }

        [
                "",
                "# Project-local core options and remaps, so the user's own",
                "# config/<Core>/<Core>.opt is never touched. global_core_options",
                "# is required: without it the per-core file wins and this is ignored.",
                "global_core_options = \"true\"",
                &format!("core_options_path = \"{}\"", opts_path.display()),
                &format!("input_remapping_directory = \"{}\"", remaps_dir.display()),
                "",
            ]
            .join("\n").to_string()
    }
    /// As [`Self::launch`], with an override config appended.
    pub fn launch_with(
        &self,
        core: &str,
        rom: &Path,
        fullscreen: bool,
        overrides: Option<&Path>,
    ) -> Result<std::process::ExitStatus> {
        self.launch_full(core, rom, fullscreen, overrides, None)
    }

    /// As [`Self::launch_with`], with an explicit shader preset.
    pub fn launch_full(
        &self,
        core: &str,
        rom: &Path,
        fullscreen: bool,
        overrides: Option<&Path>,
        shader: Option<&Path>,
    ) -> Result<std::process::ExitStatus> {
        let mut cmd = self.launch_command_full(core, rom, fullscreen, overrides, shader)?;
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
