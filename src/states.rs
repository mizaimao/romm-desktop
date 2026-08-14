//! The save states a game has, as something you can look at and pick from.
//!
//! They were already being synced with the server, which meant the app knew
//! about them and never showed one. A state is the most valuable thing in the
//! folder — it is the only record of where you actually are in a game — and the
//! only way back to it was to launch the game, open RetroArch's menu, and guess
//! which numbered slot was the right one.
//!
//! RetroArch writes a PNG beside each state when asked to, named for the state
//! file with `.png` on the end. That picture is what makes the difference
//! between a list of slot numbers and a shelf you can read at a glance, so the
//! launcher turns the setting on and this module goes looking for them.
//!
//! Nothing here writes. Deleting or renaming a state is the emulator's business
//! and getting it wrong destroys the one thing that cannot be downloaded again.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;

use crate::cache::Cache;
use crate::coremap::CoreMap;
use crate::saves::{self, Kind, Resolution};

/// One save state, ready to be shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot {
    /// RetroArch's own name for the slot: `"0"`, `"3"`, or `"auto"`.
    pub slot: String,
    /// What to call it on screen.
    pub label: String,
    pub path: PathBuf,
    /// The picture RetroArch saved with it, if it saved one. States made before
    /// the setting was turned on have none, and there is no way to produce one
    /// after the fact — the frame is not in the file.
    pub thumb: Option<PathBuf>,
    /// Seconds since the epoch, or `None` if the filesystem would not say.
    pub modified: Option<u64>,
    pub size: u64,
    /// Which core wrote it. Two cores for the same console keep separate state
    /// folders and their states are not interchangeable, so a shelf that merged
    /// them would offer states that cannot be loaded.
    pub core: String,
}

impl Slot {
    /// `auto` sorts first, then the numbered slots in order.
    ///
    /// The autosave is where RetroArch put you when you last quit, so it is
    /// almost always the one meant by "carry on" — and it is the one nobody
    /// makes deliberately, so it is also the one nobody remembers the number
    /// of.
    fn order(&self) -> (u8, u32) {
        match self.slot.as_str() {
            "auto" => (0, 0),
            n => (1, n.parse().unwrap_or(u32::MAX)),
        }
    }

    /// The `--entryslot` argument RetroArch wants, or `None` for the autosave,
    /// which has no number and is loaded a different way.
    pub fn entry_slot(&self) -> Option<u32> {
        self.slot.parse().ok()
    }
}

/// Every state belonging to `fs_name`, newest-looking first.
///
/// Reuses the save scanner rather than walking the folder again: it already
/// knows how each core names its directory, how to split a slot off a
/// filename, and which of two cores' copies of one game is the current one.
pub fn shelf(ra_root: &Path, cache: &Cache, map: &CoreMap, fs_name: &str) -> Result<Vec<Slot>> {
    let stem = Path::new(fs_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(fs_name);
    let found = saves::scan_for_stem(ra_root, cache, map, stem)?;

    let mut out: Vec<Slot> = found
        .into_iter()
        // Canonical only: a state written by a superseded core cannot be loaded
        // by the one this app will launch, so offering it is offering a button
        // that fails.
        .filter(|c| c.kind == Kind::State && c.canonical)
        .filter(|c| !matches!(c.resolution, Resolution::Unmatched))
        .map(|c| {
            let thumb = thumb_for(&c.path);
            Slot {
                label: label_for(&c.slot),
                slot: c.slot,
                modified: modified_at(&c.path),
                size: c.size,
                core: c.core_dir,
                thumb,
                path: c.path,
            }
        })
        .collect();

    out.sort_by_key(|s| s.order());
    Ok(out)
}

/// RetroArch names the picture after the whole state file, extension included:
/// `Zelda.state3` becomes `Zelda.state3.png`.
fn thumb_for(state: &Path) -> Option<PathBuf> {
    let mut name = state.file_name()?.to_os_string();
    name.push(".png");
    let png = state.with_file_name(name);
    png.is_file().then_some(png)
}

fn modified_at(path: &Path) -> Option<u64> {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
}

/// What to call a slot on screen.
///
/// "Slot 0" is RetroArch's name for it and means nothing to anybody; it is the
/// one you get by pressing the save-state key without thinking about slots at
/// all, which makes it the quick one.
fn label_for(slot: &str) -> String {
    match slot {
        "auto" => "Where you left off".to_owned(),
        "0" => "Quick slot".to_owned(),
        n => format!("Slot {n}"),
    }
}

/// How long ago, in words, for a timestamp in seconds since the epoch.
///
/// Rounded hard on purpose. The question a shelf answers is "is this the one
/// from last night or the one from two years ago", and a precise figure invites
/// reading a precision that a filesystem timestamp does not have.
pub fn ago(then: u64, now: SystemTime) -> String {
    let now = now.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    // A state stamped in the future is a clock that was wrong, not a prediction.
    let secs = now.saturating_sub(then);
    match secs {
        0..=90 => "just now".to_owned(),
        s if s < 3600 => format!("{} min ago", s / 60),
        s if s < 7200 => "an hour ago".to_owned(),
        s if s < 86_400 => format!("{} hours ago", s / 3600),
        s if s < 172_800 => "yesterday".to_owned(),
        s if s < 2_592_000 => format!("{} days ago", s / 86_400),
        s if s < 5_184_000 => "a month ago".to_owned(),
        s if s < 31_536_000 => format!("{} months ago", s / 2_592_000),
        s if s < 63_072_000 => "a year ago".to_owned(),
        s => format!("{} years ago", s / 31_536_000),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(secs: u64) -> SystemTime {
        UNIX_EPOCH + std::time::Duration::from_secs(secs)
    }

    #[test]
    fn the_autosave_comes_first_and_the_numbered_slots_in_order() {
        let mut slots: Vec<Slot> = ["3", "0", "auto", "10", "1"]
            .iter()
            .map(|s| Slot {
                slot: (*s).to_owned(),
                label: label_for(s),
                path: PathBuf::new(),
                thumb: None,
                modified: None,
                size: 0,
                core: "snes9x".into(),
            })
            .collect();
        slots.sort_by_key(|s| s.order());
        let order: Vec<&str> = slots.iter().map(|s| s.slot.as_str()).collect();
        // 10 after 1, not between 0 and 3: the slot is a number, and sorting it
        // as text is how slot 10 ends up looking like the second one you made.
        assert_eq!(order, ["auto", "0", "1", "3", "10"]);
    }

    #[test]
    fn slots_are_named_for_what_they_are_rather_than_numbered() {
        assert_eq!(label_for("auto"), "Where you left off");
        assert_eq!(label_for("0"), "Quick slot");
        assert_eq!(label_for("7"), "Slot 7");
    }

    /// The autosave has no number, and asking RetroArch to enter slot "auto"
    /// is asking for a slot that does not exist.
    #[test]
    fn only_numbered_slots_can_be_entered_directly() {
        let auto = Slot {
            slot: "auto".into(),
            label: String::new(),
            path: PathBuf::new(),
            thumb: None,
            modified: None,
            size: 0,
            core: String::new(),
        };
        assert_eq!(auto.entry_slot(), None);
        assert_eq!(Slot { slot: "4".into(), ..auto }.entry_slot(), Some(4));
    }

    #[test]
    fn how_long_ago_is_readable_at_every_scale() {
        let now = at(1_000_000_000);
        assert_eq!(ago(1_000_000_000, now), "just now");
        assert_eq!(ago(999_998_200, now), "30 min ago");
        assert_eq!(ago(999_989_200, now), "3 hours ago");
        assert_eq!(ago(999_900_000, now), "yesterday");
        assert_eq!(ago(999_400_000, now), "6 days ago");
        assert_eq!(ago(990_000_000, now), "3 months ago");
        assert_eq!(ago(900_000_000, now), "3 years ago");
    }

    /// A file stamped in the future is a clock that was wrong. Saturating
    /// rather than wrapping, because the alternative is "584942417355 years
    /// ago" on a state saved on a machine whose clock is a minute fast.
    #[test]
    fn a_timestamp_from_the_future_does_not_wrap_around() {
        assert_eq!(ago(2_000_000_000, at(1_000_000_000)), "just now");
    }

    #[test]
    fn the_picture_is_named_after_the_whole_state_file() {
        // Same shape as the cache tests: a named folder under the system temp
        // dir, so a failing run leaves something you can go and look at.
        let dir = std::env::temp_dir().join("romm-states-thumb-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let state = dir.join("Zelda.state3");
        std::fs::write(&state, b"x").unwrap();
        assert_eq!(thumb_for(&state), None, "no picture, no claim of one");

        let png = dir.join("Zelda.state3.png");
        std::fs::write(&png, b"x").unwrap();
        assert_eq!(thumb_for(&state), Some(png));

        // Not `with_extension`, which would look for `Zelda.png` and find the
        // one belonging to a completely different thing.
        std::fs::write(dir.join("Zelda.png"), b"x").unwrap();
        let state2 = dir.join("Zelda.state9");
        std::fs::write(&state2, b"x").unwrap();
        assert_eq!(thumb_for(&state2), None);
    }
}
