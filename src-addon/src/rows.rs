//! Turning the catalogue into something with a cursor in it.
//!
//! The patches tab is built by *reading the device*, not by declaring what
//! ought to be true. A row opens at whatever `state()` reports, so a device
//! somebody else set up, or one that has just taken a KNULLI update, tells you
//! where it actually stands the moment you open the app.

use crate::model::{Page, Row};
use crate::patch::{Patch, State};

/// The sync tab: status you can read at the top, then the things you can set
/// going.
pub fn sync(server: Option<&str>, last: Option<&str>) -> Page {
    Page::new(vec![
        Row::fact("server", "Server", server.unwrap_or("not configured")),
        Row::fact("last", "Last sync", last.unwrap_or("never")),
        Row::action(
            "push",
            "Push saves up",
            "Uploads game saves newer than the server's copy. Where both changed since the \
             last sync it stops and asks which to keep rather than picking one.",
            "—",
        ),
        Row::action(
            "pull",
            "Pull saves down",
            "Downloads saves newer than this device's copy, with the same question when both \
             have moved.",
            "—",
        ),
        Row::action(
            "offline",
            "Take games offline",
            "Downloads chosen games from the server onto the card so they play with no \
             network.",
            "—",
        ),
    ])
}

/// The patches tab, read back off the device.
pub fn patches(patches: &[Patch]) -> Page {
    Page::new(
        patches
            .iter()
            .map(|patch| {
                let live = match patch.state() {
                    State::At(i) => Some(i),
                    State::Changed => None,
                };
                Row::dial(patch.id, patch.title, patch.detail, patch.option_names(), live)
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalogue;
    use crate::patch::Paths;

    fn scratch(name: &str) -> Paths {
        let dir = std::env::temp_dir().join(format!("moose-rows-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Paths::new(dir)
    }

    #[test]
    fn nothing_is_queued_before_anything_is_touched() {
        // Opening the app must not propose a single change, whatever it finds.
        let paths = scratch("fresh");
        assert!(patches(&catalogue::all(&paths)).pending().is_empty());
        assert!(sync(None, None).pending().is_empty());
    }

    #[test]
    fn the_menu_opens_at_what_the_device_actually_is() {
        // Apply something behind the app's back, then build the menu: the row
        // has to come up already showing it, or the first thing the user does
        // is turn a dial that was already where they wanted it.
        let paths = scratch("reads-back");
        let all = catalogue::all(&paths);
        let hotkeys = all.iter().find(|p| p.id == "hotkeys").unwrap();
        hotkeys.apply(1).unwrap();

        let page = patches(&catalogue::all(&paths));
        let row = page.rows.iter().find(|r| r.id == "hotkeys").unwrap();
        assert_eq!(row.value(), "ON");
        assert!(!row.pending());
    }

    #[test]
    fn a_file_edited_behind_our_back_shows_as_changed() {
        let paths = scratch("drifted");
        let all = catalogue::all(&paths);
        all.iter().find(|p| p.id == "hotkeys").unwrap().apply(1).unwrap();
        // Somebody, or an update, rewrites the block.
        let conf = paths.knulli_conf();
        let text = std::fs::read_to_string(&conf).unwrap();
        std::fs::write(&conf, text.replace("global.retroarch", "# global.retroarch")).unwrap();

        let page = patches(&catalogue::all(&paths));
        let row = page.rows.iter().find(|r| r.id == "hotkeys").unwrap();
        assert!(row.adrift());
        assert_eq!(row.value(), "changed");
        assert!(!row.pending(), "still not queued until it is chosen");
    }

    #[test]
    fn every_patch_says_what_it_changes() {
        // The detail line is the only place the file it writes is recorded,
        // and an empty one makes the row unundoable by hand.
        let paths = scratch("details");
        for row in patches(&catalogue::all(&paths)).rows {
            assert!(
                row.detail.len() > 40,
                "{} needs a detail line saying what it touches",
                row.id
            );
        }
    }

    #[test]
    fn the_sync_tab_opens_on_something_you_can_press() {
        // Its first two rows are facts; the cursor has to have skipped them.
        assert!(sync(None, None).selected().is_some_and(|r| r.selectable()));
    }
}
