// Where the app's files are, and how it finds them.
//
// Config, cache, core map and library are all addressed relative to one
// directory, and where that is depends on how the app was started. This used
// to live in the Tauri layer, which meant the SDL front end opened
// `cache.sqlite3` relative to whatever directory it happened to be launched
// from — and `Cache::open` creates the file, so instead of failing it made an
// empty database and reported a library of nothing.
//
// Nothing here is created and nothing is written to the home directory.

use std::path::{Path, PathBuf};

/// Find the data root and make it the working directory.
///
/// Config, cache, core map and library are all addressed relative to one
/// directory, but where that is depends on how the app was started:
///
/// * a config.toml the user put beside the executable, or in the cwd
/// * a dev checkout — the repo itself, found by walking up for the core map
/// * otherwise the executable's own directory
///
/// Nothing is created and nothing is written to the home directory. See
/// [`choose`], which holds the ordering and is where the tests are.
pub fn anchor() {
    let cwd = std::env::current_dir().ok();
    let exe = std::env::current_exe().ok();
    match choose(cwd.as_deref(), exe.as_deref(), &|p| p.is_file()) {
        Some(root) => {
            let _ = std::env::set_current_dir(&root);
        }
        None => eprintln!(
            "warning: could not locate the executable; leaving the working directory alone"
        ),
    }
}

const MARKER: &str = "data/esde-core-map.json";
const CONFIG: &str = "config.toml";

/// Decide the data root. Split out from [`anchor`] because the
/// *order* is the part that was wrong, and a function that calls
/// `set_current_dir` cannot be tested — the working directory is per-process,
/// so tests would fight each other.
///
/// `is_file` is injected for the same reason: the interesting cases are layouts
/// nobody has on disk (a Windows portable install, a `.app` in /Applications).
pub fn choose(
    cwd: Option<&Path>,
    exe: Option<&Path>,
    is_file: &dyn Fn(&Path) -> bool,
) -> Option<PathBuf> {
    let exe_dir = exe.and_then(app_dir);

    // 1. A config.toml beside the executable, or in the working directory.
    //
    // This is what a portable install looks like and what everyone expects of
    // one: unzip the exe, drop config.toml next to it, run it. Without this the
    // Windows build ignored that file completely — it anchored to %USERPROFILE%
    // \RomM and looked for the config there, so a config sitting right beside
    // the exe was never on any path it consulted.
    //
    // Checked before the marker search because it is the more specific signal:
    // a config.toml is somewhere a user deliberately put a file, whereas the
    // marker only says "a checkout is somewhere above us".
    for dir in [cwd, exe_dir.as_deref()].into_iter().flatten() {
        if is_file(&dir.join(CONFIG)) {
            return Some(dir.to_path_buf());
        }
    }

    // 2. A source checkout, if we are running from one.
    let ancestors = cwd
        .into_iter()
        .flat_map(Path::ancestors)
        .chain(exe.into_iter().flat_map(Path::ancestors));
    for root in ancestors {
        if is_file(&root.join(MARKER)) {
            return Some(root.to_path_buf());
        }
    }

    // 3. The executable's own directory, and nowhere else.
    //
    // The app lives where it was put. It does not create a folder in the home
    // directory, and nothing is written from here at all — the core map is
    // compiled into the binary (`CoreMap::load_or_embedded`), so no file has to
    // exist on disk before startup.
    exe_dir
}

/// The directory a user would say the app is "in".
///
/// On macOS the executable is buried at `RomM-Desktop.app/Contents/MacOS/`,
/// which is inside the signed bundle: writing there breaks the signature and is
/// wiped on update. The directory holding the `.app` is the equivalent of the
/// folder a loose `.exe` sits in, so that is what gets used.
pub fn app_dir(exe: &Path) -> Option<PathBuf> {
    let dir = exe.parent()?;
    let mut cur = dir;
    while let Some(parent) = cur.parent() {
        if cur.extension().is_some_and(|e| e.eq_ignore_ascii_case("app")) {
            return Some(parent.to_path_buf());
        }
        cur = parent;
    }
    Some(dir.to_path_buf())
}


#[cfg(test)]
mod tests {
    use super::*;

    /// holding the `.app` is the app's location as far as data goes — the
    /// equivalent of the folder a loose .exe sits in.
    #[test]
    fn a_macos_bundle_resolves_to_the_directory_holding_it() {
        assert_eq!(
            app_dir(Path::new("/Applications/RomM-Desktop.app/Contents/MacOS/romm-gui")),
            Some(PathBuf::from("/Applications"))
        );
        assert_eq!(
            app_dir(Path::new(
                "/Users/frank/Projects/romm-desktop/RomM-Desktop.app/Contents/MacOS/romm-gui"
            )),
            Some(PathBuf::from("/Users/frank/Projects/romm-desktop"))
        );
    }

    /// A loose executable — the Windows and Linux shape — anchors to its own
    /// directory. Unzip it anywhere, drop a config.toml beside it, run it.
    #[test]
    fn a_loose_executable_anchors_beside_itself() {
        assert_eq!(
            app_dir(Path::new("D:/Emulators/RomM/romm-desktop.exe")),
            Some(PathBuf::from("D:/Emulators/RomM"))
        );
        assert_eq!(
            app_dir(Path::new("/opt/romm/romm-desktop")),
            Some(PathBuf::from("/opt/romm"))
        );
    }

    /// A directory merely *containing* the string "app" is not a bundle; only a
    /// `.app` extension counts, or an install under /home/apps/ would anchor a
    /// level too high.
    #[test]
    fn only_a_dot_app_extension_counts_as_a_bundle() {
        assert_eq!(
            app_dir(Path::new("/home/frank/apps/romm-desktop")),
            Some(PathBuf::from("/home/frank/apps"))
        );
        assert_eq!(
            app_dir(Path::new("/srv/appdata/romm/romm-desktop")),
            Some(PathBuf::from("/srv/appdata/romm"))
        );
    }

    /// A set of paths that "exist", for driving `choose_data_root` over layouts
    /// nobody has on this disk.
    fn exists(paths: &[&str]) -> impl Fn(&Path) -> bool + use<> {
        let owned: Vec<String> = paths.iter().map(|p| (*p).to_owned()).collect();
        move |p: &Path| owned.iter().any(|o| Path::new(o) == p)
    }

    /// The reported bug, as a test: a portable Windows install with its config
    /// beside the exe, launched from a working directory that has nothing to do
    /// with it. This used to land on %USERPROFILE%\RomM, where the config was
    /// never on any path the app consulted.
    ///
    /// The checkout marker above the working directory is what gives this test
    /// teeth. Without it the answer is right either way — step 3 also returns
    /// the executable's directory — so the ordering would not actually be under
    /// test. A mutation run (delete the config check, watch this still pass)
    /// is what surfaced that.
    #[test]
    fn a_config_beside_the_executable_wins() {
        let root = choose(
            Some(Path::new("C:/Users/frank/checkout/sub")),
            Some(Path::new("D:/Emulators/RomM/romm-desktop.exe")),
            &exists(&[
                "D:/Emulators/RomM/config.toml",
                // A checkout above the cwd, which wins if the config is
                // consulted second instead of first.
                "C:/Users/frank/checkout/data/esde-core-map.json",
            ]),
        );
        assert_eq!(root, Some(PathBuf::from("D:/Emulators/RomM")));
    }

    /// The working directory is checked before the executable, so running from
    /// a configured folder uses that config rather than the app's own.
    #[test]
    fn the_working_directory_is_preferred_over_the_executables() {
        let root = choose(
            Some(Path::new("/srv/romm-live")),
            Some(Path::new("/opt/romm/romm-desktop")),
            &exists(&["/srv/romm-live/config.toml", "/opt/romm/config.toml"]),
        );
        assert_eq!(root, Some(PathBuf::from("/srv/romm-live")));
    }

    /// A config.toml is a deliberate act; the core map only says a checkout is
    /// somewhere above us. The specific signal has to win, or a developer with
    /// a checkout above their portable install gets the wrong data directory.
    #[test]
    fn a_config_beats_a_checkout_found_further_up() {
        let root = choose(
            Some(Path::new("/home/frank/Projects/romm-desktop/portable")),
            Some(Path::new("/home/frank/Projects/romm-desktop/portable/romm-desktop")),
            &exists(&[
                "/home/frank/Projects/romm-desktop/portable/config.toml",
                "/home/frank/Projects/romm-desktop/data/esde-core-map.json",
            ]),
        );
        assert_eq!(
            root,
            Some(PathBuf::from("/home/frank/Projects/romm-desktop/portable"))
        );
    }

    /// With no config anywhere, a checkout above the working directory is used
    /// — this is what makes `cargo run` from a subdirectory work.
    #[test]
    fn a_checkout_is_found_by_walking_up() {
        let root = choose(
            Some(Path::new("/home/frank/Projects/romm-desktop/src-tauri")),
            Some(Path::new("/home/frank/Projects/romm-desktop/target/debug/romm-gui")),
            &exists(&["/home/frank/Projects/romm-desktop/data/esde-core-map.json"]),
        );
        assert_eq!(root, Some(PathBuf::from("/home/frank/Projects/romm-desktop")));
    }

    /// Nothing configured and no checkout: the app lives where it was put. The
    /// previous behaviour — inventing ~/RomM and writing a core map into it —
    /// is what this asserts is gone.
    #[test]
    fn with_nothing_to_go_on_it_anchors_beside_the_app_not_in_home() {
        let root = choose(
            Some(Path::new("/")),
            Some(Path::new("/Applications/RomM-Desktop.app/Contents/MacOS/romm-gui")),
            &exists(&[]),
        );
        assert_eq!(
            root,
            Some(PathBuf::from("/Applications")),
            "the bundle resolves to the folder holding it, and never to $HOME"
        );
        assert!(
            !format!("{root:?}").contains("RomM/"),
            "no invented data folder"
        );
    }

    /// Without an executable path there is nothing to anchor to, and leaving the
    /// working directory alone beats guessing.
    #[test]
    fn no_executable_and_no_markers_changes_nothing() {
        assert_eq!(choose(Some(Path::new("/tmp")), None, &exists(&[])), None);
    }
}
