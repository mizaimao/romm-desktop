# Parked

Still worth doing, deliberately not being done now. In the order I would take
them.

## 1. Push a Windows build and find out what is broken

Everything since 0.1.12 has run only on macOS: save states, play history,
sorting, four controllers, the shader fix, the window placement. Four rounds of
fixes have accumulated blind.

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
