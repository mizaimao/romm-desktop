//! Locating and launching a RetroArch install.
//!
//! Deliberately does not assume `/Applications/RetroArch.app`: this machine's
//! install lives elsewhere and runs in portable mode, which is the layout we
//! target. See PLAN.md §6 for how portable mode resolves directories.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::padprofile;
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

# ---- Windows: window blacks out while being resized ----
# These three used to be forced on Windows and made it worse on an Nvidia card
# with G-Sync: d3d11 with "sync to exact content framerate" settles on a
# different refresh rate every launch. Try them one at a time if resizing still
# flashes, starting with the driver.
# video_driver = "d3d11"
# vrr_runloop_enable = "true"
# video_max_swapchain_images = "2"

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

/// How much of the display the game window fills.
///
/// Not all of it. A window exactly the size of the work area is the same size
/// as a maximised one, and on both macOS and Windows that ends up flush against
/// an edge with no way to grab the title bar — so a sliver is left, which also
/// makes it obvious at a glance that this is a window and the desktop is still
/// behind it.
const SCREEN_FILL: f32 = 0.94;

/// Where the game window should go: the monitor's own origin and size, in the
/// units the desktop uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Screen {
    /// Top-left of this monitor, in the desktop's coordinates, y counting down.
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    /// Height of the primary monitor. Needed only on macOS, where window
    /// coordinates count up from the bottom of *that* screen — see below.
    pub primary_height: u32,
}

/// Space the menu bar takes at the top of a macOS screen.
///
/// A window placed under it is pushed down by the window server, which then
/// makes the bottom of the window hang off the screen. Leaving the room costs
/// nothing and is the difference between "as tall as the screen" and "as tall
/// as the screen, with the bottom inch missing".
const MENU_BAR: u32 = 38;

/// Open the game window in the top-left of the screen the library is on, as
/// tall as that screen allows.
///
/// Two things here were found by measuring rather than by reading, because both
/// are silent when wrong.
///
/// The first is the setting names. `video_window_width` and
/// `video_window_height` are what the documentation says, and RetroArch 1.20
/// does not have them -- they are absent from a config it wrote itself, which
/// writes every setting it knows. Asking for a 2406-wide window through those
/// produced a 720-wide one, because the size came from `video_scale` and
/// nothing had been overridden at all. The keys that work are
/// `video_windowed_position_width` and `_height`, and they are only read when
/// `video_window_save_positions` is on.
///
/// The second is what the coordinates mean. Asking for y = 0 on an
/// 1169-point-tall screen put the window's *bottom* at the bottom of the
/// screen: RetroArch passes the value to Cocoa, whose origin is the bottom-left
/// of the primary display with y counting upwards. So "the top of this monitor"
/// is not 0, and on a second monitor it is not even a positive number in the
/// obvious direction. The arithmetic below is that conversion, and it is why
/// the primary monitor's height has to travel with the rest.
pub fn window_lines(
    screen: Option<Screen>,
    aspect: Option<crate::aspect::Shape>,
    decorations: bool,
) -> String {
    let Some(s) = screen else {
        return String::new();
    };
    if s.width == 0 || s.height == 0 {
        return String::new();
    }

    // As tall as the screen allows, less the menu bar. Width is left a little
    // short of the full screen: a window exactly as wide as the display has no
    // edge left to grab, and every console here is squarer than a modern
    // monitor anyway, so the extra width would be black bars.
    let (top_gap, bottom_gap) = if cfg!(target_os = "macos") {
        (MENU_BAR, 0)
    } else {
        (0, 0)
    };
    // Negative coordinates do not survive the trip. Asking for x = -378 — the
    // left edge of a monitor sitting up and to the left of the built-in one —
    // put the window at x = 2142, most of it off the right-hand side of the
    // desktop, which is the "still at the right edge" this kept producing.
    // Asking for x = 0 on the same monitor landed exactly at 0. So the setting
    // is read as unsigned somewhere between here and the window.
    //
    // Clamping at zero rather than refusing: on a screen that starts left of
    // the origin, x = 0 is still a point on that screen, so the window opens
    // where it was asked to open — just not flush against the left edge. The
    // width comes down to match, or it would run off the far side.
    let left = s.x.max(0);
    let lost_x = (left - s.x).max(0) as u32;
    let usable_w = s.width.saturating_sub(lost_x);

    // Same for the vertical, in Cocoa's terms: the distance from the bottom of
    // the primary display up to the bottom edge of the window.
    let want_y = if cfg!(target_os = "macos") {
        s.primary_height as i32 - s.y - s.height as i32
    } else {
        s.y
    };
    let bottom = want_y.max(0);
    let lost_y = (bottom - want_y).max(0) as u32;
    let usable_h = s.height.saturating_sub(lost_y);

    let mut h = usable_h.saturating_sub(top_gap + bottom_gap);
    let mut w = (usable_w as f32 * SCREEN_FILL) as u32;

    // Shaped like the game, when the platform has one shape. RetroArch keeps
    // the picture's proportions inside whatever window it is given, so a
    // window of the wrong shape is a window with black bars in it — and on a
    // maximised one those bars are large. Giving it a window of the right shape
    // leaves nothing over to put a bar in.
    let mut shape = String::new();
    if let Some(a) = aspect {
        let (fw, fh) = crate::aspect::fit(w, h, a.ratio);
        w = fw;
        h = fh;

        // And tell RetroArch to draw at that same shape.
        //
        // Sizing the window alone was half a fix. The default here is square
        // pixel, which is the frame buffer's shape rather than the
        // television's: a Neo Geo frame is 320x224, which is 10:7, and it was
        // meant to be seen as 4:3. So the window was 4:3, the picture was
        // 10:7, and RetroArch did the only thing it could and put bars in the
        // difference. They agree now, and there is no difference to fill.
        //
        // Index 20 is "Config", which means "use video_aspect_ratio" — exact
        // for any ratio, where the numbered entries only cover a fixed list.
        // Only where every game on the platform really is this shape. Arcade
        // is sized to a 4:3 cabinet but drawn at whatever the game is, because
        // a rotated vertical shooter in that same cabinet is 3:4 and forcing
        // it to 4:3 would stretch it rather than letterbox it.
        if a.exact {
            shape = format!(
            "\n# Draw at the same shape as the window, so there is nothing left\n\
             # over to letterbox. 20 is \"Config\": use the ratio below.\n\
             aspect_ratio_index = \"20\"\n\
             video_aspect_ratio = \"{a:.6}\"\n\
             video_aspect_ratio_auto = \"false\"\n",
                a = a.ratio
            );
        }
    }

    // A screen so far off to the left that clamping leaves nothing worth
    // opening. Better to write no geometry at all and let RetroArch use its own
    // than to ask for a window nobody could play in.
    if w < 640 || h < 480 {
        return String::new();
    }

    let x = left;
    let y = bottom;

    let chrome = if decorations {
        ""
    } else {
        "# No title bar. The way out is the controller combination or Escape.\n\
         video_window_show_decorations = \"false\"\n"
    };
    format!(
        "{shape}{chrome}\n# Top-left of the screen the library is on, as tall as it goes.\n\
         video_fullscreen = \"false\"\n\
         # On: this is what makes the size below be read at all.\n\
         video_window_save_positions = \"true\"\n\
         video_windowed_position_width = \"{w}\"\n\
         video_windowed_position_height = \"{h}\"\n\
         video_windowed_position_x = \"{x}\"\n\
         video_windowed_position_y = \"{y}\"\n\
         # The names the documentation gives, which RetroArch 1.20 does not\n\
         # have. Kept for builds that do.\n\
         video_window_custom_size_enable = \"true\"\n\
         video_window_width = \"{w}\"\n\
         video_window_height = \"{h}\"\n\
         # And the ceiling, which is separate and silently wins. A RetroArch\n\
         # set up on a 1080p monitor keeps auto_width_max = 1920 forever, so on\n\
         # a 4K screen the window lands at a quarter of the area no matter what\n\
         # size was asked for -- which looks exactly like the size being\n\
         # ignored.\n\
         video_window_auto_width_max = \"{w}\"\n\
         video_window_auto_height_max = \"{h}\"\n"
    )
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

# The shader chosen here is the shader that runs.
#
# RetroArch looks for a preset of its own beside each core -- config/<Core>/
# <Core>.slangp -- and that one wins over `video_shader` without saying so.
# One left behind by a handheld, or by pressing \"save core preset\" once, meant
# every NES game came up in crt-royale no matter what this app asked for, and
# every attempt to change it in Settings did nothing. Those files are left
# alone; they are just not consulted.
auto_shaders_enable = \"false\"

# Mouse wired to the light gun controls.
#
# RetroArch keeps gun binds apart from pad binds and ships them unbound, so a
# gun game aims with the pointer -- that part is read directly -- and then the
# trigger does nothing. These cost nothing when no gun is in use: they only
# apply to a port a core has been told holds a gun, which is off unless it is
# switched on for that system in Settings -> Emulators.
#
# Left button fires, right button shoots off-screen (how most gun games
# reload), middle is Start.
input_player1_mouse_index = \"0\"
input_player1_gun_trigger_mbtn = \"1\"
input_player1_gun_offscreen_shot_mbtn = \"2\"
input_player1_gun_start_mbtn = \"3\"
# A save state with no picture is a slot number, and a slot number is not
# something anybody remembers. RetroArch writes the frame beside the state when
# asked; nothing can produce one after the fact.
savestate_thumbnail_enable = \"true\"

input_player2_mouse_index = \"0\"
input_player2_gun_trigger_mbtn = \"1\"
input_player2_gun_offscreen_shot_mbtn = \"2\"
input_player2_gun_start_mbtn = \"3\"

";

    /// Windows-only additions, appended to `OVERRIDES` there.
    ///
    /// Empty of video settings, and that is the fix rather than an omission.
    ///
    /// This block used to force `video_driver = "d3d11"`, `vrr_runloop_enable`
    /// and a two-image swapchain, to stop a window blacking out while being
    /// resized on a 144 Hz display. It did not stop it. What it did do was
    /// take a machine already set to d3d12 and move it to d3d11, which is the
    /// one driver with a known VRR problem: on an Nvidia card with G-Sync,
    /// d3d11 plus "sync to exact content framerate" -- which is what
    /// `vrr_runloop_enable` is -- negotiates a refresh rate several Hz below
    /// the display's, differently on each launch (libretro/RetroArch#14513).
    /// A timing figure that changes per launch is a driver reinitialisation
    /// waiting to happen, and a reinitialisation is a black screen.
    ///
    /// So the whole block is gone. RetroArch's own config decides the video
    /// driver, as it did before, and `retroarch-user.cfg` carries the old
    /// settings commented out for anyone whose display did want them.
    #[cfg(target_os = "windows")]
    const OVERRIDES_OS: &str = "";

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
        self.write_overrides_full(dir, user_config, "", None, true, false)
    }

    /// As above, with `extra` (per-platform shader settings) inserted before
    /// the user's file — so the user can still override even those.
    pub fn write_overrides_full(
        &self,
        dir: &Path,
        user_config: Option<&Path>,
        extra: &str,
        pad: Option<&str>,
        mirror_players: bool,
        autofire: bool,
    ) -> Result<PathBuf> {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("creating {}", dir.display()))?;
        let mut body = Self::OVERRIDES.to_owned();
        body.push_str(Self::OVERRIDES_OS);
        body.push_str(&self.hotkeys(pad));
        body.push_str(&self.players(pad, mirror_players));
        if autofire {
            body.push_str(&self.autofire(pad));
        }
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

    /// The controller hotkey block for `device` (the connected pad's reported
    /// name, if the frontend knows it).
    ///
    /// Generated per launch rather than shipped as fixed numbers: RetroArch
    /// hotkeys take raw driver indices, which differ per controller *and* per
    /// operating system. See [`crate::padprofile`].
    pub fn hotkeys(&self, device: Option<&str>) -> String {
        // Only ever a profile RetroArch itself wrote, or one built in for a pad
        // somebody has actually reported.
        //
        // There used to be a generic Xbox table behind these, used whenever
        // nothing matched. That is worse than nothing. A hotkey index that is
        // wrong does not fail quietly — it binds the modifier to a button or
        // stick used constantly in play, so the menu opens or the game quits
        // while you are moving. A guess is only safe when being wrong is
        // cheap, and here it is not.
        // Both places RetroArch keeps them: beside the binary for a portable
        // install, and in its user-data directory for a normal one. The second
        // is where anything it learned about the connected pad ends up, so
        // searching only the first found shipped defaults and missed the
        // profile that is actually in use.
        let roots = [self.root.join("autoconfig"), self.data_dir().join("autoconfig")];
        // Ask RetroArch which driver the pad is on rather than guessing an
        // order. The same controller is numbered differently under each, so a
        // profile from the wrong directory is valid, plausible and completely
        // wrong — which is exactly how the modifier ended up on a stick.
        let driver = padprofile::configured_driver(&[
            self.data_dir().join("retroarch.cfg"),
            self.root.join("retroarch.cfg"),
        ]);
        match padprofile::find_with_driver(&roots, device, driver.as_deref())
            .or_else(|| padprofile::known(device))
        {
            Some(profile) => padprofile::hotkey_block(&profile),
            None => padprofile::no_profile_note(&roots, device),
        }
    }

    /// Auto-fire: the shot button repeats while held.
    ///
    /// Arcade shooters were built around a cabinet button you hammered, and a
    /// run of Metal Slug is a few thousand presses. Every home port of these
    /// games since has offered auto-fire; the arcade originals cannot, because
    /// the hardware had no such thing.
    ///
    /// The arrangement, and why it is this way round: the bottom face button
    /// becomes the repeating one, because that is the one already under the
    /// thumb and the one that hurts. Single shots move to the top face button,
    /// which on a Neo Geo four-button layout is button D — unused by Metal
    /// Slug and by most of the run-and-gun games, so nothing is displaced. A
    /// game that does use all four still works: the top button keeps sending
    /// the shot, it simply no longer sends D.
    ///
    /// RetroArch's "single button (hold)" turbo mode is what does the
    /// repeating: it pulses `input_turbo_default_button` — RetroPad B, which
    /// every arcade core maps to the primary fire — while the turbo button is
    /// held.
    pub fn autofire(&self, device: Option<&str>) -> String {
        let roots = [self.root.join("autoconfig"), self.data_dir().join("autoconfig")];
        let driver = padprofile::configured_driver(&[
            self.data_dir().join("retroarch.cfg"),
            self.root.join("retroarch.cfg"),
        ]);
        let Some(profile) = padprofile::find_with_driver(&roots, device, driver.as_deref())
            .or_else(|| padprofile::known(device))
        else {
            return String::new();
        };
        // Both buttons have to be known. Half of this arrangement is worse
        // than none of it: a shot button that repeats with nowhere to fire a
        // single shot from is a game you cannot aim carefully in.
        let (Some(hold), Some(single)) = (
            profile.get(padprofile::Physical::A),
            profile.get(padprofile::Physical::Y),
        ) else {
            return String::new();
        };
        format!(
            "\n# ---- Auto-fire ----\n\
             # The bottom face button repeats while held; single shots move to\n\
             # the top one, which these games do not use. Turned off per game\n\
             # in Settings, or for everything with autofire = false.\n\
             input_turbo_mode = \"3\"\n\
             input_turbo_default_button = \"0\"\n\
             input_turbo_period = \"8\"\n\
             input_turbo_duty_cycle = \"4\"\n\
             {}\n{}\n",
            hold.line("player1_turbo"),
            single.line("player1_b"),
        )
    }

    /// The player-port block: how many ports, the stick standing in for the
    /// d-pad on each, and optionally players 2-4 bound like player 1.
    ///
    /// Resolved from the same profile as [`Self::hotkeys`] and by the same
    /// route, because mirroring is only meaningful against the pad the
    /// frontend can actually see.
    pub fn players(&self, device: Option<&str>, mirror: bool) -> String {
        let roots = [self.root.join("autoconfig"), self.data_dir().join("autoconfig")];
        let driver = padprofile::configured_driver(&[
            self.data_dir().join("retroarch.cfg"),
            self.root.join("retroarch.cfg"),
        ]);
        let profile = padprofile::find_with_driver(&roots, device, driver.as_deref())
            .or_else(|| padprofile::known(device));
        crate::players::config_lines(profile.as_ref(), mirror)
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
        self.launch_command_full(core, rom, fullscreen, overrides, None, None)
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
        entry_slot: Option<u32>,
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
        // Start in a save state rather than at the title screen. RetroArch
        // takes the slot number, not a path, which is why the shelf keeps
        // RetroArch's own slot names rather than inventing its own.
        if let Some(slot) = entry_slot {
            cmd.arg(format!("--entryslot={slot}"));
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
        self.launch_full(core, rom, fullscreen, overrides, None, None)
    }

    /// As [`Self::launch_with`], with an explicit shader preset.
    pub fn launch_full(
        &self,
        core: &str,
        rom: &Path,
        fullscreen: bool,
        overrides: Option<&Path>,
        shader: Option<&Path>,
        entry_slot: Option<u32>,
    ) -> Result<std::process::ExitStatus> {
        let mut cmd =
            self.launch_command_full(core, rom, fullscreen, overrides, shader, entry_slot)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn fake(root: &Path) -> RetroArch {
        RetroArch {
            root: root.to_path_buf(),
            binary: root.join("retroarch"),
            portable: false,
            system_override: None,
        }
    }

    /// Give a scratch RetroArch an autoconfig profile, in every driver
    /// directory this OS searches, so the test works the same on all three.
    ///
    /// Needed now that no profile means no hotkeys: a test that wants hotkeys
    /// has to supply the thing they are derived from, which is also a more
    /// honest reflection of how this works in reality.
    fn with_autoconfig(root: &Path) {
        const PROFILE: &str = r#"
input_driver = "test"
input_device = "Xbox Wireless Controller"
input_b_btn = "0"
input_a_btn = "1"
input_y_btn = "2"
input_x_btn = "3"
input_l_btn = "4"
input_r_btn = "5"
input_select_btn = "6"
input_start_btn = "7"
input_up_btn = "h0up"
input_down_btn = "h0down"
input_left_btn = "h0left"
input_right_btn = "h0right"
input_l2_axis = "+2"
input_r2_axis = "+5"
"#;
        for driver in ["mfi", "hid", "xinput", "dinput", "sdl2", "udev", "linuxraw"] {
            let dir = root.join("autoconfig").join(driver);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("xbox.cfg"), PROFILE).unwrap();
        }
    }

    /// Each test gets its own directory under the OS temp dir, named for the
    /// test, so a failure leaves inspectable output and runs cannot collide.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("romm-desktop-test-{name}"));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).expect("creating a temp dir");
        dir
    }

    /// The user's own file is appended last, because `--appendconfig` gives the
    /// final assignment of a key. This is the promise the README makes: we never
    /// modify their retroarch.cfg and never win an argument with it.
    /// One monitor, at the desktop origin.
    fn screen(width: u32, height: u32) -> Screen {
        Screen { x: 0, y: 0, width, height, primary_height: height }
    }

    fn val(out: &str, key: &str) -> String {
        out.lines()
            .find(|l| l.starts_with(&format!("{key} = ")))
            .and_then(|l| l.split('"').nth(1))
            .unwrap_or_else(|| panic!("{key} is not written:\n{out}"))
            .to_owned()
    }

    /// The keys the documentation names do not exist in RetroArch 1.20, and a
    /// window asked for through them came out at 720 pixels wide because the
    /// size fell back to `video_scale`. These are the ones that work, and they
    /// are only read when saved positions are on.
    #[test]
    fn the_size_is_written_under_the_names_retroarch_actually_reads() {
        let out = window_lines(Some(screen(2560, 1440)), None, true);
        assert_eq!(val(&out, "video_window_save_positions"), "true");
        assert_eq!(val(&out, "video_windowed_position_width"), "2406");
        // The full height of the screen less the menu bar, not a fraction of
        // it: vertical space is the thing being maximised.
        assert_eq!(
            val(&out, "video_windowed_position_height"),
            if cfg!(target_os = "macos") { "1402" } else { "1440" }
        );
        // And the documented names too, for builds that have them.
        assert_eq!(val(&out, "video_window_custom_size_enable"), "true");
        assert_eq!(val(&out, "video_window_width"), "2406");
    }

    /// Top-left, and as tall as the screen allows.
    ///
    /// The vertical coordinate is the part that cannot be reasoned out: asking
    /// for y = 0 on an 1169-tall screen put the window's *bottom* at the bottom
    /// of the screen, because RetroArch hands the number to Cocoa, whose origin
    /// is the bottom-left of the primary display. On the primary monitor that
    /// makes the top of the screen y = 0 only because the window is exactly as
    /// tall as the space below the menu bar.
    #[test]
    #[cfg(target_os = "macos")]
    fn the_window_sits_under_the_menu_bar_at_the_top_left() {
        let out = window_lines(Some(screen(1800, 1169)), None, true);
        assert_eq!(val(&out, "video_windowed_position_x"), "0");
        assert_eq!(val(&out, "video_windowed_position_height"), "1131");
        // 1169 - 0 - (1131 + 38) = 0: the window's bottom edge on the bottom
        // of the screen, its top just under the menu bar.
        assert_eq!(val(&out, "video_windowed_position_y"), "0");
    }

    /// A monitor above the primary one has negative coordinates going down and
    /// positive ones going up. Getting this backwards puts the window off the
    /// bottom of the desktop, which is where it went.
    ///
    /// The horizontal is the other half: a monitor that starts left of the
    /// origin cannot be addressed by its own left edge, because a negative x
    /// does not survive — asking for -378 put the window at 2142, most of it
    /// off the right of the desktop. Zero is still a point on that screen, so
    /// that is where it goes, with the width brought in to match.
    #[test]
    #[cfg(target_os = "macos")]
    fn a_monitor_above_and_left_of_the_primary_one_is_still_reachable() {
        let out = window_lines(Some(Screen {
            x: -380,
            y: -1440,
            width: 2560,
            height: 1440,
            primary_height: 1169,
        }), None, true);
        assert_eq!(val(&out, "video_windowed_position_x"), "0");
        // 2560 wide starting 380 to the left of the origin leaves 2180 usable.
        assert_eq!(val(&out, "video_windowed_position_width"), "2049");
        assert_eq!(val(&out, "video_windowed_position_height"), "1402");
        // 1169 - (-1440) - 1440 = 1169: the window's bottom edge level with the
        // top of the primary screen, which is the bottom of this one.
        assert_eq!(val(&out, "video_windowed_position_y"), "1169");
    }

    /// A screen entirely left of the origin still gets a window, and the
    /// arithmetic must not underflow on the way — these are unsigned widths.
    #[test]
    #[cfg(target_os = "macos")]
    fn a_screen_far_off_to_the_left_does_not_underflow() {
        let out = window_lines(Some(Screen {
            x: -3000,
            y: 0,
            width: 1920,
            height: 1080,
            primary_height: 1169,
        }), None, true);
        assert_eq!(out, "", "a window nobody could play in is not worth asking for");
    }

    /// Everywhere else y counts down from the top, so a monitor's own origin
    /// is already the answer.
    #[test]
    #[cfg(not(target_os = "macos"))]
    fn elsewhere_the_monitor_origin_is_the_position() {
        let out = window_lines(Some(Screen {
            x: 1920,
            y: 0,
            width: 2560,
            height: 1440,
            primary_height: 1080,
        }), None, true);
        assert_eq!(val(&out, "video_windowed_position_x"), "1920");
        assert_eq!(val(&out, "video_windowed_position_y"), "0");
        assert_eq!(val(&out, "video_windowed_position_height"), "1440");
    }

    /// The ceiling is a separate setting that silently wins. RetroArch keeps
    /// whatever `auto_width_max` it was last configured with, so one set up on
    /// a 1080p monitor caps every window at 1920x1080 forever — and on a 4K
    /// screen that is a quarter of the area, which reads as the requested size
    /// being ignored rather than capped.
    #[test]
    fn the_size_is_written_alongside_the_ceiling_that_would_cap_it() {
        let out = window_lines(Some(screen(3840, 2160)), None, true);
        assert_eq!(
            val(&out, "video_windowed_position_width"),
            val(&out, "video_window_auto_width_max"),
            "a ceiling below the requested size caps it"
        );
        assert_eq!(
            val(&out, "video_windowed_position_height"),
            val(&out, "video_window_auto_height_max")
        );
    }

    /// A window the exact size of the display is a maximised one with no title
    /// bar left to grab. It has to come in from the edges.
    #[test]
    fn the_window_is_a_little_smaller_than_the_display() {
        let out = window_lines(Some(screen(1000, 1000)), None, true);
        let w: u32 = val(&out, "video_windowed_position_width").parse().unwrap();
        assert!(w < 1000, "width {w} fills the display, leaving no edge to grab");
        assert!(w > 850, "width {w} wastes the screen");
        // Height is the one thing that is not held back: "as much vertical
        // space as the screen has" is the whole point of the placement.
        let h: u32 = val(&out, "video_windowed_position_height").parse().unwrap();
        assert!(h >= 1000 - super::MENU_BAR, "height {h} is not the full screen");
    }

    /// The black bars are the window being the wrong shape, and on a maximised
    /// window they are large: a 3:2 handheld game on a 16:10 laptop screen
    /// leaves a column down each side wider than the game was tall on the
    /// original hardware. A window shaped like the game has nothing over.
    #[test]
    fn a_window_shaped_like_the_game_leaves_no_room_for_a_bar() {
        // A wide screen, where a maximised window and a 3:2 game plainly
        // disagree. On a 16:10 laptop the two happen to land within a few
        // percent of each other, which would make this pass on a coincidence.
        let bare = window_lines(Some(screen(2560, 1440)), None, true);
        let fitted = window_lines(Some(screen(2560, 1440)), crate::aspect::of("gba"), true);

        let read = |out: &str, k: &str| -> f32 { val(out, k).parse().unwrap() };
        let shape = |out: &str| {
            read(out, "video_windowed_position_width") / read(out, "video_windowed_position_height")
        };
        assert!((shape(&fitted) - 1.5).abs() < 0.01, "{fitted}");
        // And it is genuinely different from the unshaped one, or the test is
        // passing on a coincidence of this particular screen.
        assert!((shape(&bare) - 1.5).abs() > 0.05, "{bare}");
        // Still inside the screen.
        assert!(read(&fitted, "video_windowed_position_width") <= 2560.0);
        assert!(read(&fitted, "video_windowed_position_height") <= 1440.0);
    }

    /// A platform nobody has mapped is left exactly as it was: no shaping of
    /// the window, and no ratio forced on the picture.
    #[test]
    fn a_platform_with_no_one_shape_is_left_alone() {
        let a = window_lines(Some(screen(1800, 1169)), crate::aspect::of("some-new-thing"), true);
        let b = window_lines(Some(screen(1800, 1169)), None, true);
        assert_eq!(a, b);
        assert!(!b.contains("aspect_ratio_index"), "{b}");
    }

    /// Sizing the window was half the job. The picture is drawn at whatever
    /// ratio RetroArch is set to, and the default is the frame buffer's shape
    /// rather than the television's — a Neo Geo frame is 320x224, which is
    /// 10:7, shown as 4:3. So a 4:3 window held a 10:7 picture and RetroArch
    /// put bars in the difference, which is exactly what a fitted window is
    /// supposed to remove.
    #[test]
    fn the_picture_is_drawn_at_the_shape_the_window_was_given() {
        let out = window_lines(Some(screen(1800, 1169)), crate::aspect::of("neogeoaes"), true);
        assert_eq!(val(&out, "aspect_ratio_index"), "20", "20 is \"use the ratio below\"");
        let asked: f32 = val(&out, "video_aspect_ratio").parse().unwrap();
        assert!((asked - 4.0 / 3.0).abs() < 0.001, "{out}");

        // And the window really is that shape, or the two still disagree.
        let w: f32 = val(&out, "video_windowed_position_width").parse().unwrap();
        let h: f32 = val(&out, "video_windowed_position_height").parse().unwrap();
        assert!((w / h - asked).abs() < 0.01, "window {w}x{h} is not {asked}");
    }

    /// RetroArch loads a preset of its own from `config/<Core>/<Core>.slangp`
    /// and that one wins over `video_shader` without saying so anywhere. One
    /// left behind by a handheld, or by pressing "save core preset" once,
    /// meant every NES game came up in crt-royale whatever this app asked for
    /// — and every attempt to change it in Settings appeared to do nothing.
    #[test]
    fn the_shader_we_choose_is_not_overruled_by_one_retroarch_finds_itself() {
        let dir = scratch("auto-shaders");
        let body =
            std::fs::read_to_string(fake(&dir).write_overrides_with(&dir, None).unwrap()).unwrap();
        assert!(
            body.contains("auto_shaders_enable = \"false\""),
            "a preset beside the core will quietly replace ours:\n{body}"
        );
    }

    /// Arcade gets the cabinet's shape for its window and nothing forced on
    /// the picture. Both halves matter: a window the width of the screen puts
    /// a black column down each side of every horizontal game, and a forced
    /// 4:3 would stretch every vertical one.
    #[test]
    fn arcade_is_given_a_cabinet_shaped_window_but_no_forced_ratio() {
        let out = window_lines(Some(screen(2560, 1440)), crate::aspect::of("arcade"), true);
        let w: f32 = val(&out, "video_windowed_position_width").parse().unwrap();
        let h: f32 = val(&out, "video_windowed_position_height").parse().unwrap();
        assert!((w / h - 4.0 / 3.0).abs() < 0.01, "window {w}x{h} is not a cabinet");
        assert!(
            !out.contains("aspect_ratio_index"),
            "a vertical shooter would be stretched to fit:\n{out}"
        );
    }

    /// Auto-fire moves two buttons and nothing else. Getting only half of it
    /// written would be worse than none: a shot button that repeats, with
    /// nowhere left to fire a single shot from, is a game you cannot aim in.
    #[test]
    fn autofire_moves_the_shot_to_the_top_button_and_repeats_the_bottom_one() {
        let dir = scratch("autofire");
        with_autoconfig(&dir);
        let out = fake(&dir).autofire(Some("Xbox Wireless Controller"));

        // RetroPad B is the primary fire in every arcade core, and mode 3 is
        // "single button (hold)" — pulse it while the turbo button is down.
        assert!(out.contains("input_turbo_mode = \"3\""), "{out}");
        assert!(out.contains("input_turbo_default_button = \"0\""), "{out}");
        assert!(out.contains("input_player1_turbo_btn"), "{out}");
        // And the single shot has somewhere to live.
        assert!(out.contains("input_player1_b_btn"), "{out}");

        // The two are different buttons, or holding one would do both.
        let val = |k: &str| {
            out.lines()
                .find(|l| l.starts_with(k))
                .and_then(|l| l.split('"').nth(1))
                .unwrap_or_default()
                .to_owned()
        };
        assert_ne!(val("input_player1_turbo_btn"), val("input_player1_b_btn"));
    }

    /// A pad nothing is known about gets nothing. Guessing a button index here
    /// does not fail quietly — it binds fire to whatever that index happens to
    /// be on the pad in someone's hands.
    #[test]
    fn autofire_writes_nothing_for_a_pad_with_no_profile() {
        let dir = scratch("autofire-unknown");
        assert_eq!(fake(&dir).autofire(Some("Some Unheard-of Pad")), "");
    }

    /// Nothing known about the display means nothing written: the emulator's
    /// own window settings are the user's, and a guess would overwrite them.
    #[test]
    fn an_unknown_display_leaves_the_window_settings_alone() {
        assert_eq!(window_lines(None, None, true), "");
        assert_eq!(window_lines(Some(screen(0, 1080)), None, true), "");
        assert_eq!(window_lines(Some(screen(1920, 0)), None, true), "");
    }

    #[test]
    fn the_users_own_settings_come_last() {
        let dir = scratch("user-last");
        with_autoconfig(&dir);
        let user = dir.join("mine.cfg");
        std::fs::write(&user, "video_driver = \"vulkan\"\n").unwrap();

        let path = fake(&dir)
            .write_overrides_full(&dir, Some(&user), "video_smooth = \"true\"\n", None, true, false)
            .unwrap();
        let body = std::fs::read_to_string(path).unwrap();

        let ours = body.find("input_enable_hotkey_btn").expect("our hotkeys");
        let extra = body.find("video_smooth").expect("the per-platform extra");
        let theirs = body.find("vulkan").expect("the user's line");
        assert!(ours < extra, "per-platform settings layer over ours");
        assert!(extra < theirs, "the user's file layers over everything");
    }

    /// A missing user config is a warning, not a failed launch — the file is
    /// optional and someone deleting it should not be unable to play.
    #[test]
    fn a_missing_user_config_does_not_fail_the_launch() {
        let dir = scratch("missing-user");
        with_autoconfig(&dir);
        let path = fake(&dir)
            .write_overrides_full(&dir, Some(&dir.join("nope.cfg")), "", None, true, false)
            .unwrap();
        assert!(std::fs::read_to_string(path).unwrap().contains("input_enable_hotkey_btn"));
    }

    /// The hotkeys are generated from the pad profile, not hardcoded.
    ///
    /// The layout is asserted symbolically: which *button* does what, with the
    /// index looked up the same way the generator looks it up. Pinning literal
    /// numbers here is what let the wrong ones ship — they looked right next to
    /// a matching comment, and were the browser's indices rather than
    /// RetroArch's.
    #[test]
    fn the_hotkeys_come_from_the_pad_profile() {
        use crate::padprofile;

        let dir = scratch("hotkeys");
        with_autoconfig(&dir);
        let body = std::fs::read_to_string(
            fake(&dir).write_overrides_full(&dir, None, "", None, true, false).unwrap(),
        )
        .unwrap();

        // The profile written above is what the block must be derived from.
        let profile = padprofile::find(&dir, None).expect("the autoconfig we just wrote");
        assert!(
            body.contains(&profile.get(padprofile::MODIFIER).unwrap().line("enable_hotkey")),
            "Select must be bound as the modifier"
        );
        for (action, button, _) in padprofile::HOTKEYS {
            let bind = profile.get(*button).expect("fallback covers every hotkey");
            assert!(
                body.contains(&bind.line(action)),
                "{action} should be on {button:?}, as {}",
                bind.line(action)
            );
        }

        // The two settings that make quit survivable.
        assert!(body.contains("quit_press_twice = \"true\""), "quit must confirm");
        assert!(
            body.contains("config_save_on_exit = \"false\""),
            "RetroArch must not write our layered settings back into the user\'s config"
        );
    }

    /// Quit must never sit on the same button as the modifier, and must never
    /// be reachable without it. Holding B and tapping A is an ordinary thing to
    /// do in a game; it should not end one.
    #[test]
    fn quit_is_not_reachable_during_normal_play() {
        use crate::padprofile::{self, Physical};

        let dir = scratch("quit-safety");
        with_autoconfig(&dir);
        let body = std::fs::read_to_string(
            fake(&dir).write_overrides_full(&dir, None, "", None, true, false).unwrap(),
        )
        .unwrap();

        let profile = padprofile::find(&dir, None).expect("the autoconfig we just wrote");
        let modifier = profile.get(padprofile::MODIFIER).unwrap();
        let quit = profile.get(Physical::A).unwrap();
        assert_ne!(modifier.value, quit.value);
        assert!(body.contains(&modifier.line("enable_hotkey")));
        // The face buttons must not be the modifier under any profile.
        for face in [Physical::A, Physical::B, Physical::X, Physical::Y] {
            let bind = profile.get(face).unwrap();
            assert_ne!(
                bind.value, modifier.value,
                "{face:?} is used constantly in play and cannot be the hotkey modifier"
            );
        }
    }

    /// Every emitted line must be a `key = value` assignment, a comment, or
    /// blank. A stray escaped newline once joined the platform block onto the
    /// preceding line, which RetroArch parses as one malformed setting.
    #[test]
    fn every_emitted_line_parses_as_a_setting() {
        let dir = scratch("well-formed");
        let path = fake(&dir).write_overrides_full(&dir, None, "", None, true, false).unwrap();
        for line in std::fs::read_to_string(path).unwrap().lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (key, _) = line.split_once(" = ").unwrap_or_else(|| panic!("malformed: {line}"));
            assert!(
                key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
                "key {key:?} has characters RetroArch will not accept"
            );
        }
    }

    /// Locating reports what it tried rather than a bare "not found", because
    /// the usual cause is an install somewhere unusual (a second drive) and the
    /// list is what tells the user to go set the path in Settings.
    #[test]
    fn a_failed_locate_names_the_paths_it_tried() {
        let missing = scratch("no-retroarch").join("nowhere");
        let err = RetroArch::locate_in(&[missing.display().to_string()])
            .expect_err("there is no RetroArch there")
            .to_string();
        assert!(err.contains(&missing.display().to_string()), "got: {err}");
        assert!(err.contains("config.toml"), "should say how to fix it: {err}");
    }

    /// The starter user config is written once and never overwritten — it holds
    /// the user's own settings after the first run.
    #[test]
    fn the_starter_user_config_is_never_overwritten() {
        let path = scratch("starter").join("user.cfg");
        assert!(RetroArch::ensure_user_config(&path).unwrap(), "written the first time");
        std::fs::write(&path, "input_driver = \"mine\"\n").unwrap();
        assert!(!RetroArch::ensure_user_config(&path).unwrap(), "left alone the second time");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "input_driver = \"mine\"\n");
    }

    /// The change this file exists to guard: with nothing to derive hotkeys
    /// from, none are written.
    ///
    /// A wrong index does not fail quietly — it puts the modifier on a button or
    /// stick used constantly in play, so the menu opens or the game quits while
    /// you are moving. That is exactly what a generic fallback table produced on
    /// a pad it did not describe.
    #[test]
    fn no_profile_means_no_hotkeys_rather_than_a_guess() {
        let dir = scratch("no-profile");
        let body = std::fs::read_to_string(
            fake(&dir).write_overrides_full(&dir, None, "", Some("Some Unusual Pad"), true, false).unwrap(),
        )
        .unwrap();

        assert!(
            !body.contains("input_enable_hotkey"),
            "no modifier may be invented: {body}"
        );
        assert!(
            !body.contains("input_exit_emulator"),
            "and certainly not quit"
        );
        // But it must say why, and where it looked.
        assert!(body.contains("No profile matched"), "{body}");
        assert!(body.contains("autoconfig"), "names the directory: {body}");
        assert!(body.contains("run RetroArch once"), "and how to fix it: {body}");
    }

    /// The note is comments only, so it cannot change any setting by accident.
    #[test]
    fn the_no_profile_note_binds_nothing() {
        let dir = scratch("no-profile-lines");
        let body = std::fs::read_to_string(
            fake(&dir).write_overrides_full(&dir, None, "", None, true, false).unwrap(),
        )
        .unwrap();
        // Only the hotkey keys: `input_overlay_enable` and friends are ordinary
        // base settings and have nothing to do with a pad profile.
        let mut names: Vec<String> = vec!["enable_hotkey".to_owned()];
        names.extend(padprofile::HOTKEYS.iter().map(|(a, _, _)| (*a).to_owned()));

        let bound: Vec<&str> = body
            .lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .filter(|l| names.iter().any(|n| l.contains(&format!("input_{n}_"))))
            .collect();
        assert!(bound.is_empty(), "no hotkey may be bound: {bound:?}");
    }
}