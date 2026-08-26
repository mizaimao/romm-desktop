//! The two lists.
//!
//! These are declarations, not behaviour: each row says what it is called,
//! what it can be set to, and one line about what it actually changes on
//! disk. Nothing here applies anything — see `model` for why that is
//! deliberate.
//!
//! The `detail` lines matter more than they look. Every one of these patches
//! was placed by hand first, and the thing that was hard was never the change
//! itself but knowing *which* file KNULLI would keep. Writing that down beside
//! each row is what stops it having to be worked out twice.

use crate::model::{Page, Row};

/// The sync tab: status you can read at the top, then the things you can set
/// going.
pub fn sync() -> Page {
    Page::new(vec![
        Row::fact("server", "Server", "not configured"),
        Row::fact("last", "Last sync", "never"),
        Row::action(
            "push",
            "Push saves up",
            "Uploads game saves newer than the server's copy. Where both \
             changed since the last sync it stops and asks which to keep.",
            "—",
        ),
        Row::action(
            "pull",
            "Pull saves down",
            "Downloads saves newer than this device's copy, with the same \
             question when both moved.",
            "—",
        ),
        Row::action(
            "offline",
            "Take games offline",
            "Downloads chosen games from the server onto the card so they \
             play with no network.",
            "—",
        ),
    ])
}

/// The patches tab. Order is roughly how likely you are to want it.
pub fn patches() -> Page {
    Page::new(vec![
        Row::choice(
            "hotkeys",
            "Hotkeys",
            "Writes the RetroArch hotkey block into knulli.conf. It has to go \
             there rather than into RetroArch's own menu because configgen \
             rewrites retroarch.cfg at every single launch, which is why \
             changes made inside RetroArch never stick.",
            &["ON", "off"],
            0,
        ),
        Row::choice(
            "shaders",
            "Shaders",
            "global.shaderset in knulli.conf. sharp-shimmerless keeps the \
             pixel grid even when the scale factor is not a whole number, \
             which at 640x480 is nearly always.",
            &["shimmerless + LCD/CRT", "shimmerless plain", "zfast", "off"],
            0,
        ),
        Row::choice(
            "bezels",
            "Bezels",
            "<system>.bezel in knulli.conf, with the artwork in \
             /userdata/decorations — the shipped packs are on the squashfs and \
             do not survive an upgrade.",
            &["off", "KNULLI default", "RomM GBA"],
            0,
        ),
        Row::choice(
            "hotkey-app",
            "L2+R2 opens this app",
            "Two lines in /userdata/system/configs/multimedia_keys.conf, which \
             S50triggerhappy prefers over anything in /etc — and /etc is on \
             the tmpfs overlay, so a rule there would be gone by the next boot.",
            &["ON", "off"],
            0,
        ),
        Row::choice(
            "es-shoulders",
            "ES ignores L2/R2",
            "A 24-line es_input.cfg holding only this device's pad, with l2 \
             and r2 dropped. Copying the shipped file across whole does not \
             work: 291 pad definitions, and ES will not start.",
            &["ON", "off"],
            0,
        ),
        Row::choice(
            "es-logo",
            "Hide the loading logo",
            "A black 1280x720 PNG over resources/logo.png, put back at every \
             boot from /boot/boot-custom.sh. ES draws that file whenever it is \
             loading — every game launch and every return — and there is no \
             setting for it.",
            &["ON", "off"],
            0,
        ),
        Row::choice(
            "boot-splash",
            "Clear the boot splash",
            "Zeroes /dev/fb0 after boot, where S03system-splash leaves the \
             KNULLI logo. Only shows if something stops drawing, but then it \
             is the whole screen.",
            &["ON", "off"],
            0,
        ),
        Row::choice(
            "never-sleep",
            "Never sleep",
            "system.batterysaver.extendedmode. Suspending after 15 minutes \
             idle drops the network and reads as a dead device. Dimming stays \
             either way.",
            &["ON", "off"],
            0,
        ),
        Row::choice(
            "gpu",
            "Graphics driver",
            "Swaps /usr/lib/libmali.so.1 at boot from /userdata/system/gpu. \
             The stock blob has no Wayland support; the g24p0 one does, and \
             the emulators behave identically on both.",
            &["stock", "wayland"],
            0,
        ),
        Row::choice(
            "fast-launch",
            "Faster game launch",
            "A resident python that imports configgen once and forks per game, \
             instead of paying the import every launch. Measured on this \
             device: 1241ms becomes 7.9ms, so roughly 3.8s down to 2.6s.",
            &["off", "ON"],
            0,
        ),
        Row::choice(
            "wifi-awake",
            "Keep Wi-Fi awake",
            "Turns off the wireless power saving that drops the connection \
             while idle. Costs battery; buys a device that answers.",
            &["off", "ON"],
            0,
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_queued_before_anything_is_touched() {
        // Opening the app must not propose a single change. Every row's
        // starting pick is what the device already is.
        assert!(sync().pending().is_empty());
        assert!(patches().pending().is_empty());
    }

    #[test]
    fn every_patch_says_what_it_changes() {
        // The detail line is the only place the file it writes is recorded,
        // and an empty one makes the row unundoable by hand.
        for row in patches().rows {
            assert!(
                row.detail.len() > 40,
                "{} needs a detail line saying what it touches",
                row.id
            );
        }
    }

    #[test]
    fn choices_have_something_to_choose_between() {
        for row in patches().rows {
            if let crate::model::Kind::Choice { options, .. } = &row.kind {
                assert!(options.len() >= 2, "{} has one option", row.id);
            }
        }
    }

    #[test]
    fn the_sync_tab_opens_on_something_you_can_press() {
        // Its first two rows are facts; the cursor has to have skipped them.
        let page = sync();
        assert!(page.selected().map(|r| r.selectable()).unwrap_or(false));
    }
}
