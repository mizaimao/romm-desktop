// What differs between the devices this app runs on.
//
// The axis of difference is the *device*, not the front end. SDL-vs-webview is
// a rendering choice and answers none of these questions:
//
//   1. Where does the library live?
//   2. Where does RetroArch keep its own files?
//   3. Which system directories mean what, and which are not ours?
//   4. How do brightness and battery work?
//
// Before this module those were answered by `#[cfg(target_os = …)]` scattered
// across `src/`, which cannot express the targets that matter. KNULLI *is*
// `target_os = "linux"` while being nothing like desktop Linux, and Android
// falls through the same `not(any(macos, windows))` branch. Both compiled and
// both behaved wrong.
//
// Selection is by Cargo feature, with the host's `target_os` as the fallback
// when no scheme is named — so an ordinary build is unchanged and the two
// schemes `target_os` cannot reach are opt-in.

use std::path::{Path, PathBuf};

use crate::esde;

pub mod android;
pub mod knulli;
pub mod linux;
pub mod macos;
pub mod windows;

/// A screen backlight that can be read and set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Brightness {
    /// Where the level is read and written, 0..=`max`.
    pub path: PathBuf,
    pub max: u32,
    /// A vendor CLI that already wraps the sysfs node, preferred when present
    /// because it also handles whatever else the device couples to backlight.
    pub helper: Option<&'static str>,
}

/// A wireless connection that can be read without spawning anything.
///
/// `/proc/net/wireless` is one line per interface and holds the link quality,
/// so a status bar can read it every couple of seconds for the cost of opening
/// a file. The SSID needs `iw`, which is a process — that belongs on a settings
/// screen, not in the corner of every frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wifi {
    pub proc_wireless: PathBuf,
    pub interface: &'static str,
    /// The largest value the quality column reports, for turning it into bars.
    pub max_quality: u32,
    /// Lists and joins networks. A process, so it is only for settings.
    ///
    /// Called as `<helper> scanlist` to look, and `<helper> enable <ssid>
    /// <key>` to join — which saves the network and waits for an address, so it
    /// can take several seconds and must not be run from the draw loop.
    pub helper: Option<&'static str>,
    /// Reads one saved value, as `<settings> <key>`. `wifi.ssid` is the network
    /// the device is set to join.
    pub settings_get: Option<&'static str>,
}

/// A battery that can be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Battery {
    /// Reads 0..=100.
    pub capacity: PathBuf,
    /// Each reads `1` when that supply is connected.
    pub charging: Vec<PathBuf>,
    pub helper: Option<&'static str>,
}

/// One target's answers.
///
/// Everything has a default that means "this device has nothing to say about
/// it", so a new scheme starts as a handful of lines rather than a wall of
/// stubs.
pub trait Platform: Sync {
    /// The scheme's name, for diagnostics and for tests that assert which one
    /// a build selected.
    fn scheme(&self) -> &'static str;

    /// Where the app's own files live — config, cache, core map.
    ///
    /// `None` means "work it out from the executable", which is
    /// [`crate::datadir::choose`] and is right everywhere the executable sits
    /// beside its data. Android is the exception: `current_exe()` there is the
    /// zygote, `/system/bin/app_process64`, and nowhere near the app.
    fn data_root(&self) -> Option<PathBuf> {
        None
    }

    /// The library layout, where the device fixes it.
    ///
    /// `None` means the user says where it is. A handheld running a known OS
    /// image does not need asking.
    fn default_library(&self) -> Option<esde::Layout> {
        None
    }

    /// Install roots to search for RetroArch, in order.
    fn retroarch_roots(&self) -> &'static [&'static str];

    /// Where RetroArch keeps `retroarch.cfg`, `config/`, `autoconfig/` and
    /// `shaders/` — which is *not* always beside the binary, and on Batocera
    /// is not the XDG location either.
    fn retroarch_data_dir(&self, install_root: &Path) -> PathBuf;

    /// Directories holding libretro cores, searched after RetroArch's own.
    fn core_dirs(&self) -> &'static [&'static str] {
        &[]
    }

    /// ES-DE system directory -> RomM slug, for names this device uses that
    /// the shipped core map does not know.
    fn system_aliases(&self) -> &'static [(&'static str, &'static str)] {
        &[]
    }

    /// Cores this device should use in preference to the shipped default.
    ///
    /// RomM platform slug -> libretro core stem. The shipped core map is ES-DE's
    /// and assumes a desktop; a quad A55 at 1.8 GHz does not run what a desktop
    /// runs, and the difference is the game being playable or not.
    fn default_cores(&self) -> &'static [(&'static str, &'static str)] {
        &[]
    }

    /// System directories to pass over in silence on this device.
    ///
    /// Distinct from "unknown": an unknown directory is reported as skipped so
    /// a missing alias can be found, whereas these are known and deliberately
    /// not wanted, so reporting them is noise.
    fn ignored_systems(&self) -> &'static [&'static str] {
        &[]
    }

    fn brightness(&self) -> Option<Brightness> {
        None
    }

    fn battery(&self) -> Option<Battery> {
        None
    }

    fn wifi(&self) -> Option<Wifi> {
        None
    }
}

/// The scheme this build selected.
///
/// A feature wins if one is set; otherwise the host's `target_os` decides, so
/// a plain `cargo build` behaves exactly as it did before this module existed.
pub fn current() -> &'static dyn Platform {
    #[cfg(feature = "knulli")]
    {
        &knulli::Knulli
    }
    #[cfg(all(feature = "android", not(feature = "knulli")))]
    {
        &android::Android
    }
    #[cfg(all(feature = "macos", not(any(feature = "knulli", feature = "android"))))]
    {
        &macos::MacOs
    }
    #[cfg(all(
        feature = "windows",
        not(any(feature = "knulli", feature = "android", feature = "macos"))
    ))]
    {
        &windows::Windows
    }
    #[cfg(all(
        feature = "linux",
        not(any(
            feature = "knulli",
            feature = "android",
            feature = "macos",
            feature = "windows"
        ))
    ))]
    {
        &linux::Linux
    }
    // No scheme named: fall back to the host, which is what every build did
    // before there were schemes.
    #[cfg(not(any(
        feature = "knulli",
        feature = "android",
        feature = "macos",
        feature = "windows",
        feature = "linux"
    )))]
    {
        #[cfg(target_os = "macos")]
        {
            &macos::MacOs
        }
        #[cfg(target_os = "windows")]
        {
            &windows::Windows
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            &linux::Linux
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every scheme has to answer the questions the app actually asks. A new
    /// one that returns an empty root list, or points its RetroArch data
    /// directory at the install root by accident, fails here rather than on
    /// the device.
    #[test]
    fn every_scheme_answers_the_basics() {
        let schemes: [&dyn Platform; 5] = [
            &macos::MacOs,
            &windows::Windows,
            &linux::Linux,
            &knulli::Knulli,
            &android::Android,
        ];
        for p in schemes {
            assert!(
                !p.scheme().is_empty(),
                "a scheme must be able to name itself"
            );
            assert!(
                !p.retroarch_roots().is_empty(),
                "{}: nowhere to look for RetroArch",
                p.scheme()
            );
        }
    }

    /// The five names are distinct, so a build cannot silently select the
    /// wrong one and still look right in a log line.
    #[test]
    fn scheme_names_are_distinct() {
        let names = [
            macos::MacOs.scheme(),
            windows::Windows.scheme(),
            linux::Linux.scheme(),
            knulli::Knulli.scheme(),
            android::Android.scheme(),
        ];
        let mut sorted = names;
        sorted.sort_unstable();
        let before = sorted.len();
        let mut deduped = sorted.to_vec();
        deduped.dedup();
        assert_eq!(deduped.len(), before, "duplicate scheme name in {names:?}");
    }

    /// KNULLI is `target_os = "linux"` and must not inherit desktop Linux's
    /// answers. This is the reason the module exists, so it is asserted
    /// directly rather than left implied.
    #[test]
    fn knulli_does_not_share_desktop_linuxs_retroarch_directory() {
        let root = Path::new("/usr");
        assert_ne!(
            knulli::Knulli.retroarch_data_dir(root),
            linux::Linux.retroarch_data_dir(root),
            "KNULLI keeps RetroArch's files outside XDG; sharing the desktop \
             answer points the app at a directory that does not exist"
        );
    }

    /// Measured on the device 2026-08-24: `/userdata/system/.config/retroarch`
    /// does not exist, `/userdata/system/configs/retroarch/` does.
    #[test]
    fn knulli_points_at_batoceras_own_retroarch_directory() {
        assert_eq!(
            knulli::Knulli.retroarch_data_dir(Path::new("/usr")),
            PathBuf::from("/userdata/system/configs/retroarch"),
        );
    }
}
