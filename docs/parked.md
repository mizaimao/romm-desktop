# Parked

Still worth doing, deliberately not being done now. In the order I would take
them. `handover.md` is the wider brief; this is just the queue.

## 1. Cheats

Parked on 2026-08-20: "I don't use cheats very often so don't bother. Park it
we will revisit."

Scoped first, so picking it up again starts from facts rather than from a
survey. Everything below was checked against the RetroArch install on this
machine, not assumed.

**The database is already there.** `RetroArch/cht/`, 58 system folders, shipped
with RetroArch. Nothing has to be fetched for the console and FBNeo cases.

**The format is flat and easy.** `cheats = "6"`, then `cheat0_desc`,
`cheat0_address`, `cheat0_enable` and a dozen more keys per cheat — 97 lines
for six cheats. Parseable and writable in the same shape.

**Three cases, not two:**

| | Files | Named by | Difficulty |
|---|---|---|---|
| FBNeo (arcade) | 71 | the romset — `1941.cht` | trivial, no matching |
| MAME 2003-Plus | **0** | — | not shipped at all |
| Consoles | thousands | No-Intro title — `2020 Super Baseball (USA).cht` | name matching |

**The arcade catch.** MAME-family cheats are not in the libretro database and
that empty folder is not a broken install: MAME uses Pugsy's `cheat.dat`, a
different XML format in the system directory, and mame2003_plus reads that
instead. So "arcade cheats" is two implementations. FBNeo covers 71 of the
2,506 arcade games here — about 3%.

**The shape of the work**, if it is picked up: a `cheats.rs` holding the parser
and the game-to-file lookup; a row in the game detail pane beside the core
picker; and the chosen cheats written into a per-game `.cht` through
`prepare_tweaks`, which already writes per-game files *and deletes them when
they empty*. That delete is not optional — a cheat left enabled in a stale file
is the same failure that kept rapid fire on the wrong button for ten rounds.

**And it must refuse under achievement hardcore mode**, which already disables
save states, rewind and fast-forward. Handing someone the means to invalidate
their own achievements silently would be worse than not having the feature.

## 2. First-run onboarding

Asked for on 2026-08-19, deliberately deferred:

> I have an idea of app on-boarding, to tell the users what they need to grab
> and to start using the app. This would be more meaningful if we have more
> users so deferr this and log it somewhere so we can revisit in the future.

Worth doing when there is a second user. The app currently assumes somebody
already knows four things that nothing on screen says:

* a `config.toml` with a server url and a token has to exist before anything
  works — `config.example.toml` documents it, but only if you find the file
* `Sync library` has to be run once or the grid is empty, which reads as broken
* console pictures need `Get console pictures` before the grid has any art
* the light gun is a per-console tick in Emulators, and does nothing until the
  core is told a gun exists — the one that prompted this, and now at least
  explained in place

The shape it wants is probably a panel on an empty library rather than a wizard:
each step says what it is for, and disappears once it is satisfied.

## 3. An ES-DE icon-set preview tab

Asked for on 2026-08-17, in these words:

> Add the following ES-DE icon sets. Actually, create a tab inside settings
> dedicated for the ES-DE icons set preview. Add these: first CODYWHEEL,
> DIAMOND, ELEGANCE, ELEMENTERIAL, ICONIC, IMMERSIVE, MERINGUE, RAZOR, RETRO
> MEGA. Later on we add more and create previews for each of them so that the
> users can preview before decide which one to download and apply.

Nine sets to start with, a settings tab of their own, and a preview of each one
*before* downloading — that last part is the point, and it is what the current
[icons] `style` control does not do: it offers five names and you find out what
they look like by picking one.

What exists to build on: `src/theme.rs` has `IconStyle` with five variants and
`src/theme_remote.rs` already fetches artwork out of four ES-DE themes and
throws the themes away. The fetch machinery is there; the set list, the tab and
the previews are not.

## 4. Per-backdrop controls

Parked mid-session with "we dedicate a session for controls-per-backdrop".
Directions for Sweep and the other directional styles, and per-style slider
ranges — Motion means something different to Static than it does to Blobs, and
one shared range serves neither well.

## 5. Japanese titles in the voted lists

`sfc` and `famicom` cannot match against the published rankings until the raw
lists in `data/community/raw/` carry `English Title | 日本語タイトル` pairs.
`norm()` handles both scripts now; the source lists only carry one.

## 6. Push a Windows build and find out what is broken

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

## 7. `install-retroarch` in the GUI

It exists as a CLI subcommand and has zero references in `src-tauri`. A fresh
machine cannot set itself up, which is the one step that still assumes someone
did something by hand first.

## 8. A "what is missing" page, built from data already collected

`docs/` holds 2,504 arcade probe verdicts, the missing-ROM list, label
mismatches, BIOS coverage and 125 core reassignments. None of it is visible in
the app. This is a page over files that already exist, not new work.

## 9. Save states from other machines

States sync with the server, but `states::shelf` reads local directories only.
A state made on the handheld is not offerable here until something pulls it
down.

## 10. Per-game shader override

The core is choosable per game; the shader is per platform only. The machinery
(`shader_overrides`) is already threaded through the launch path — this is a
control, not a feature.

## 11. Per-game aspect for vertical arcade games

Arcade gets a 4:3 window and keeps whatever shape the game reports, so a
vertical shooter is pillarboxed. The probe logs recorded each game's geometry,
so the real shape could come from data rather than a guess.

## 12. Paging on the controller

Lost when the stick clicks became sorting. PageUp/PageDown still work on the
keyboard.

## 13. ScreenScraper developer ID

Requested, never arrived. Scraping still goes through the server's own account.

## 14. Windowing the middle column

The arcade list is 2,506 games and every one of them is inserted into the
document on each platform switch. The oldest performance item here. The per-row listeners are gone — one
delegated listener on the container serves them all and survives a redraw,
which took the redraw from 53ms to 34ms in jsdom — but the nodes themselves are
the remaining cost and no amount of listener work touches them.

Drawing only the rows on screen and filling in as you scroll is the fix. It is
a real change: the cursor, the "remember where this tab was" scroll position,
the pad navigation and the lazy cover observer all currently assume every row
exists.

## 15. The two rough edges left in three columns

Coming back up from a collection's games still uses the one-pane trail, and
Continue playing is only drawn in one of the two panes. Neither is wrong on
screen often enough to have been worth stopping for, and both want the same
answer: what "back" means in a window where nothing is ever replaced.

## 16. RetroArch's per-core overrides, at the source

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
