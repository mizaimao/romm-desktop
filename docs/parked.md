# Parked

Still worth doing, deliberately not being done now. In the order I would take
them. `handover.md` is the wider brief; this is just the queue.

## 0. The SDL front end

Parked on 2026-08-22, in Frank's words:

> Like I said I don't like that minilong handheld and therefore don't need to
> have an arm-linux build therefore kills half of the reasons to go to SDL. …
> Becuase sofar what I can see the visual difference between SDL and Tauri is
> day and night, maybe two generations. … Yes park this SDL branch idea and
> commit current works. I may occosionaly come back and see if it's possible
> to do SDL.

The branch is `sdl-port`, unmerged, and it builds. What is on it: a GL
renderer with rounded corners and frosted panels, `cosmic-text` with script
detection and font fallback, the shader backdrop, box art with a bounded
cache, mouse and pad input, Sofa and Desk, the console grid, the game wall,
the game list, the Continue playing strip, the detail pane, and a rendering
test suite that reads pixels back.

What was never written: search, collections, sort and filter menus, the header
controls, an on-screen keyboard, launching a game, and everything the settings
window does.

Nothing is stranded. `layout.rs`, `script.rs`, `gamelist.rs`, `gamesort.rs`,
`gamefilter.rs`, `pagefilter.rs`, `pickorder.rs`, `gridnav.rs`, `padpoll.rs`,
`rowwindow.rs`, `binds.rs` and `datadir.rs` all live in the core crate and
serve whatever draws them — the SDL work is what forced most of them out of
the webview in the first place.

The two reasons to come back: an ARM-Linux handheld he actually likes, or
Tauri's memory turning out to be unfixable on a 2 GB Android device. See
`memory.md` for where that stands.

## 1. An Android build

Parked on 2026-08-21, once the SDL front end existed and the question became
askable:

> If we have this SDL build, how far are we from getting an Android build? …
> I think it's doable because ES-DE's Android port got it figured out alright
> and there are a shit load of Android frontends doing it already, we can just
> piggyback from their open source repos. … Maybe I will drop that Linux
> support, it turned out that I don't really like this Miniloong device.

**Browsing is close.** SDL2 has first-class Android support, `cosmic-text` is
pure Rust and reads `/system/fonts` (where Android's own Noto CJK lives, which
also settles the Han unification question there), and `rusqlite` is already on
the `bundled` feature so SQLite compiles for ARM. What is missing is a
toolchain and a shell: `cargo-ndk`, a thin Gradle project carrying SDL's own
`SDLActivity.java` to load our `.so`, and a TLS story — `rustls-platform-
verifier` wants JNI on Android. A fortnight, roughly.

**Playing is a different project.** The core launches RetroArch as a
subprocess (`src/retroarch.rs`), and on Android that is an Intent to
RetroArch's app instead. Add scoped storage for ROMs and saves and it is not a
port of the launch path, it is a second one. `src/launch.rs` plans the launch
and does not spawn anything, which is the half that survives.

**Do not start from ES-DE.** Its Android port is not open source. The ones to
read are the frontends that are — Daijishō and Lemuroid both drive RetroArch
by Intent from Kotlin, which is exactly the piece we would be writing.

**If Linux is dropped, this changes shape entirely**: the handheld work in
`handheld-device.md` and `handheld-frontend.md` task 3 is all RK3566, and an
Android handheld would replace it rather than join it. Worth deciding before
picking either up again.

## 2. Cheats

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

## 3. First-run onboarding

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

## 4. An ES-DE icon-set preview tab

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

## 5. Per-backdrop controls

Parked mid-session with "we dedicate a session for controls-per-backdrop".
Directions for Sweep and the other directional styles, and per-style slider
ranges — Motion means something different to Static than it does to Blobs, and
one shared range serves neither well.

## 6. Japanese titles in the voted lists

`sfc` and `famicom` cannot match against the published rankings until the raw
lists in `data/community/raw/` carry `English Title | 日本語タイトル` pairs.
`norm()` handles both scripts now; the source lists only carry one.

## 7. Push a Windows build and find out what is broken

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

## 8. `install-retroarch` in the GUI

It exists as a CLI subcommand and has zero references in `src-tauri`. A fresh
machine cannot set itself up, which is the one step that still assumes someone
did something by hand first.

## 9. A "what is missing" page, built from data already collected

`docs/` holds 2,504 arcade probe verdicts, the missing-ROM list, label
mismatches, BIOS coverage and 125 core reassignments. None of it is visible in
the app. This is a page over files that already exist, not new work.

## 10. Save states from other machines

States sync with the server, but `states::shelf` reads local directories only.
A state made on the handheld is not offerable here until something pulls it
down.

## 11. Per-game shader override

The core is choosable per game; the shader is per platform only. The machinery
(`shader_overrides`) is already threaded through the launch path — this is a
control, not a feature.

## 12. Per-game aspect for vertical arcade games

Arcade gets a 4:3 window and keeps whatever shape the game reports, so a
vertical shooter is pillarboxed. The probe logs recorded each game's geometry,
so the real shape could come from data rather than a guess.

## 13. Paging on the controller

Lost when the stick clicks became sorting. PageUp/PageDown still work on the
keyboard.

## 14. ScreenScraper developer ID

Requested, never arrived. Scraping still goes through the server's own account.

## 15. Windowing the middle column — **done in 0.2.606**

Both halves. Covers are released once a card is well off screen, and a flat
list over 400 rows draws only the band around the viewport: 2,506 cards to 100,
`renderRows` 447ms to 21ms. See `docs/handheld-frontend.md` task 2 for what had
to change to get there. The rest of this entry is what it looked like before.

### What it looked like before

The arcade list is 2,506 games and every one of them is inserted into the
document on each platform switch. The oldest performance item here. The per-row listeners are gone — one
delegated listener on the container serves them all and survives a redraw,
which took the redraw from 53ms to 34ms in jsdom — but the nodes themselves are
the remaining cost and no amount of listener work touches them.

It is also where the memory is. Measured 2026-08-20: the WebKit WebContent
process sits at 578 MB of a ~671 MB total, because every cover scrolled past
stays decoded — the observer unobserves once loaded and nothing releases the
image. Windowing fixes both at once. See `docs/handheld-frontend.md` task 2.

Drawing only the rows on screen and filling in as you scroll is the fix. It is
a real change: the cursor, the "remember where this tab was" scroll position,
the pad navigation and the lazy cover observer all currently assume every row
exists.

**One more number, measured 2026-08-20 while chasing a report that browsing a
large platform felt laggy.** Parked with "forget about the laggy thing" — kept
here so nobody measures it twice. A worktree at 98a3b8b and one at 0.2.504,
same jsdom probe, 2,506 rows:

| | before the refactor | after |
|---|--:|--:|
| entering a platform | 412ms | 394ms |
| one cursor move | 106ms | 109ms |
| `renderRows` | ~340ms | ~380ms |

So the front-end refactor did not cause it. **73ms of every cursor move is
`selectRom` drawing the info pane, and it sends three commands per move** —
`rom_detail`, `game_cores`, `game_states` — while a held direction repeats nine
times a second. Debouncing the pane, so it draws for where the cursor stopped
rather than for every row it passed over, is a smaller and more separable job
than windowing and would probably be felt first.

## 16. The two rough edges left in three columns

Coming back up from a collection's games still uses the one-pane trail, and
Continue playing is only drawn in one of the two panes. Neither is wrong on
screen often enough to have been worth stopping for, and both want the same
answer: what "back" means in a window where nothing is ever replaced.

## 17. RetroArch's per-core overrides, at the source

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
