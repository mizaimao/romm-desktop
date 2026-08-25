// Ports and Tools, exactly as KNULLI has them.
//
// These are not consoles and they are not a RomM platform. A port is a shell
// script — PortMaster's, mostly — sitting in `/userdata/roms/ports` beside a
// folder of its own data, and KNULLI shows the scripts and hides the folders.
// `esde::scan` deliberately skips both directories, because scanning them as a
// system invents games and hands them to a core that cannot run them; this is
// the other half of that decision rather than a change to it.
//
// Read off the device on 2026-08-25:
//
//   * the launchable things are `.sh` and `.squashfs`
//   * `gamelist.xml` holds `<path>`, `<name>` and a relative `<image>`
//   * everything else in the folder is one of those scripts' data
//   * KNULLI launches with
//     `emulatorlauncher -system <system> -rom <path>` — its argument parser
//     marks exactly those two as required and the rest optional
//
// `tools` is empty on this device, `ports` has six and `emulators` has two, which
// is why an empty folder shows nothing at all rather than an empty screen.

use std::path::{Path, PathBuf};

/// One launchable script.
pub struct Port {
    pub name: String,
    pub path: PathBuf,
    /// Box art or a screenshot, when the gamelist named one that exists.
    pub image: Option<PathBuf>,
}

/// The folders KNULLI keeps these in, and what it calls them.
///
/// Three, not two. `emulators` holds PPSSPP and ScummVM on this device — the
/// standalone emulators that are launched as scripts rather than as a libretro
/// core, so they belong here rather than in the Emulators *settings* pane,
/// which is about which core runs a console. `tools` is empty on this device
/// and shows nothing.
pub const FOLDERS: [(&str, &str); 3] = [
    ("ports", "Ports"),
    ("tools", "Tools"),
    ("emulators", "Emulators"),
];

/// Everything launchable in one folder, by name.
///
/// Empty when the folder is missing or holds nothing — the caller shows no row
/// at all in that case, which is what makes an empty `tools` invisible rather
/// than a screen saying nothing is here.
pub fn scan(roms: &Path, system: &str) -> Vec<Port> {
    let dir = roms.join(system);
    let names = gamelist_names(&dir.join("gamelist.xml"));

    let mut out: Vec<Port> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let ext = path.extension()?.to_str()?.to_ascii_lowercase();
            if ext != "sh" && ext != "squashfs" {
                return None;
            }
            let file = path.file_name()?.to_str()?.to_owned();
            let stem = path.file_stem()?.to_str()?.to_owned();
            let (name, image) = names
                .iter()
                .find(|(p, _, _)| *p == file)
                .map(|(_, n, i)| (n.clone(), i.clone()))
                .unwrap_or((stem, None));
            Some(Port {
                name,
                image: image
                    .map(|i| dir.join(i.trim_start_matches("./")))
                    .filter(|p| p.is_file()),
                path,
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

/// `(file name, display name, image)` out of a gamelist.
///
/// Hand-rolled for the same reason `esde.rs` hand-rolls its own: the format is
/// flat, and the only awkward part is that `<path>` is relative and usually
/// prefixed `./`, so it is reduced to a file name for matching.
fn gamelist_names(path: &Path) -> Vec<(String, String, Option<String>)> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for block in text.split("<game>").skip(1) {
        let block = block.split("</game>").next().unwrap_or(block);
        let Some(raw) = tag(block, "path") else {
            continue;
        };
        let file = raw.rsplit(['/', '\\']).next().unwrap_or(&raw).to_owned();
        let name = tag(block, "name").unwrap_or_else(|| file.clone());
        out.push((file, name, tag(block, "image")));
    }
    out
}

fn tag(block: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let start = block.find(&open)? + open.len();
    let end = block[start..].find(&close)? + start;
    let value = block[start..end].trim();
    (!value.is_empty()).then(|| value.to_owned())
}

/// Launch one, the way KNULLI does.
///
/// Through `emulatorlauncher` rather than by running the script directly: that
/// is what KNULLI's own `es_systems.cfg` invokes, and it sets up the controller
/// configuration a PortMaster script expects to find already in place. Running
/// the `.sh` ourselves would work for some and quietly not for others.
pub fn launch(system: &str, port: &Port) -> std::io::Result<std::process::ExitStatus> {
    std::process::Command::new("emulatorlauncher")
        .arg("-system")
        .arg(system)
        .arg("-rom")
        .arg(&port.path)
        .status()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real gamelist from `/userdata/roms/ports`, 2026-08-25.
    const REAL: &str = r#"<?xml version="1.0"?>
<gameList>
	<game>
		<path>./Install.PortMaster.sh</path>
		<name>Install.PortMaster</name>
		<playcount>1</playcount>
	</game>
	<game>
		<path>./yatka.sh</path>
		<name>yatka</name>
		<image>./yatka/screenshot.jpg</image>
	</game>
</gameList>"#;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("romm-ports-{name}"));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(dir.join("ports")).unwrap();
        dir
    }

    #[test]
    fn the_real_gamelist_gives_names_and_pictures() {
        let got = {
            let dir = scratch("gamelist");
            let path = dir.join("ports/gamelist.xml");
            std::fs::write(&path, REAL).unwrap();
            gamelist_names(&path)
        };
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].0, "Install.PortMaster.sh", "the ./ prefix was kept");
        assert_eq!(got[1].1, "yatka");
        assert_eq!(got[1].2.as_deref(), Some("./yatka/screenshot.jpg"));
        assert_eq!(got[0].2, None, "an absent image is not an empty one");
    }

    /// Scripts are the games; the folders beside them are those scripts' data.
    ///
    /// KNULLI's own `ports` folder holds five scripts and six directories, and
    /// listing the directories would offer six things that cannot be launched.
    #[test]
    fn only_scripts_are_listed_and_folders_are_not() {
        let dir = scratch("scan");
        let ports = dir.join("ports");
        for f in ["yatka.sh", "Echo Chamber.sh", "thing.squashfs", "_info.txt"] {
            std::fs::write(ports.join(f), "#!/bin/sh\n").unwrap();
        }
        std::fs::create_dir_all(ports.join("yatka")).unwrap();
        std::fs::write(ports.join("gamelist.xml"), REAL).unwrap();

        let found = scan(&dir, "ports");
        let names: Vec<&str> = found.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            names,
            ["Echo Chamber", "thing", "yatka"],
            "a folder or a text file was offered as a port"
        );
    }

    /// A name with a space in it survives — KNULLI's own list has
    /// "Friday Night Funkin.sh" and "Dueling Dragons.sh" in it.
    #[test]
    fn a_name_with_spaces_is_one_port() {
        let dir = scratch("spaces");
        std::fs::write(dir.join("ports/Friday Night Funkin.sh"), "x").unwrap();
        let found = scan(&dir, "ports");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "Friday Night Funkin");
    }

    /// An image the gamelist names but which is not on disk is no image, not a
    /// broken one.
    #[test]
    fn a_missing_picture_is_no_picture() {
        let dir = scratch("noimage");
        std::fs::write(dir.join("ports/yatka.sh"), "x").unwrap();
        std::fs::write(dir.join("ports/gamelist.xml"), REAL).unwrap();
        let found = scan(&dir, "ports");
        assert_eq!(found[0].name, "yatka");
        assert_eq!(found[0].image, None, "a path that does not exist was kept");
    }

    /// All three of KNULLI's script folders, not just the two obvious ones.
    ///
    /// `emulators` holds PPSSPP and ScummVM — standalone emulators launched as
    /// scripts. Leaving it out is why two things on the device had nowhere to
    /// appear.
    #[test]
    fn all_three_script_folders_are_offered() {
        let ids: Vec<&str> = FOLDERS.iter().map(|(id, _)| *id).collect();
        assert_eq!(ids, ["ports", "tools", "emulators"]);
    }

    /// An empty folder is nothing to show. `tools` is empty on this device, and
    /// a Tools row leading to an empty screen is worse than no row.
    #[test]
    fn an_empty_or_missing_folder_offers_nothing() {
        let dir = scratch("empty");
        assert!(scan(&dir, "ports").is_empty());
        assert!(scan(&dir, "tools").is_empty(), "a folder that is not there");
    }
}
