# Parked

Still worth doing, deliberately not being done now. In the order I would take
them.

## 1. Push a Windows build and find out what is broken

Everything since 0.1.12 has run only on macOS: save states, play history,
sorting, four controllers, the shader fix, the window placement, the whole
three-column window, and the About tab's `open_link`, which shells out to
`open` on macOS and `xdg-open` everywhere else and has never been run on
Windows — where neither exists.

One of them is a known gap rather than a guess. `src/macdisplay.rs` is
`#[cfg(target_os = "macos")]`, so the screen geometry on Windows still comes
from the toolkit's pixel-size-divided-by-scale-factor — which is exactly the
arithmetic that put the game window at the right-hand edge of a scaled macOS
display. Windows display scaling at 125% or 150% is the same shape of problem
and is the default on most laptops.

## 2. `install-retroarch` in the GUI

It exists as a CLI subcommand and has zero references in `src-tauri`. A fresh
machine cannot set itself up, which is the one step that still assumes someone
did something by hand first.

## 3. A "what is missing" page, built from data already collected

`docs/` holds 2,504 arcade probe verdicts, the missing-ROM list, label
mismatches, BIOS coverage and 125 core reassignments. None of it is visible in
the app. This is a page over files that already exist, not new work.

## 4. Save states from other machines

States sync with the server, but `states::shelf` reads local directories only.
A state made on the handheld is not offerable here until something pulls it
down.

## 5. Per-game shader override

The core is choosable per game; the shader is per platform only. The machinery
(`shader_overrides`) is already threaded through the launch path — this is a
control, not a feature.

## 6. Per-game aspect for vertical arcade games

Arcade gets a 4:3 window and keeps whatever shape the game reports, so a
vertical shooter is pillarboxed. The probe logs recorded each game's geometry,
so the real shape could come from data rather than a guess.

## 7. Paging on the controller

Lost when the stick clicks became sorting. PageUp/PageDown still work on the
keyboard.

## 8. ScreenScraper developer ID

Requested, never arrived. Scraping still goes through the server's own account.

## 9. Windowing the middle column

The arcade list is 2,506 games and every one of them is inserted into the
document on each platform switch. The per-row listeners are gone — one
delegated listener on the container serves them all and survives a redraw,
which took the redraw from 53ms to 34ms in jsdom — but the nodes themselves are
the remaining cost and no amount of listener work touches them.

Drawing only the rows on screen and filling in as you scroll is the fix. It is
a real change: the cursor, the "remember where this tab was" scroll position,
the pad navigation and the lazy cover observer all currently assume every row
exists.

## 10. The two rough edges left in three columns

Coming back up from a collection's games still uses the one-pane trail, and
Continue playing is only drawn in one of the two panes. Neither is wrong on
screen often enough to have been worth stopping for, and both want the same
answer: what "back" means in a window where nothing is ever replaced.

## 11. RetroArch's per-core overrides, at the source

`config/<Core>/<Core>.cfg` is loaded after everything passed with
`--appendconfig`, so anything in there wins over what a launch was asked for.
Three of those files — Geolith, FinalBurn Neo, MAME 2003-Plus — held four lines
each of turbo settings and cost ten rounds of chasing rapid fire that was being
replaced on every launch.

The app detects the collision now, names the keys in the launch notes, and
switches override loading off for that run. What it does not do is offer to
clean them up: they are the user's files, in the user's RetroArch directory,
and a frontend that edits those has broken the promise the README makes. A
"three files are fighting your settings — remove them?" prompt would be the
honest version, and it is not built.

## 12. Windowing the middle column, still

Unchanged from 9 and now the oldest performance item: 2,506 rows are still
inserted on every platform switch.
