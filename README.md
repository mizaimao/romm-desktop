# romm-desktop

A native desktop client for a self-hosted [RomM](https://romm.app) server. Browse
your library, download ROMs on demand, and launch them in RetroArch — with a
gamepad, from the sofa, without a browser.

`PLAN.md` holds the design notes and the reasoning behind specific decisions;
this file is how to run it.

## Why not the web UI

RomM's in-browser player picks its emulator core based on whether
`SharedArrayBuffer` exists, which needs cross-origin isolation **and** a secure
context. Over plain `http://` on a LAN there is no secure context, so it falls
back to a single-threaded core and anything past the 16-bit era struggles. A
native client sidesteps that entirely and uses the RetroArch you already have.

## Building

Rust 1.97.1 — pinned in `rust-toolchain.toml`, so `rustup` will fetch it for you.

```sh
npm ci                      # Tauri CLI only; the UI has no build step
cargo build --release       # CLI and TUI
./scripts/build-macos.sh    # macOS app bundle, into the repo root
```

On Linux the GUI additionally needs `libwebkit2gtk-4.1-dev`, `libgtk-3-dev`,
`librsvg2-dev`, `libayatana-appindicator3-dev` and `patchelf`.

Linux and Windows binaries come from CI — tag a commit `v*`. **macOS is
deliberately absent from CI**: an unsigned bundle arrives quarantined and needs
mounting, dragging and an `xattr` call before it will open, so build it on the
machine that runs it.

## Configuring

```sh
cp config.example.toml config.toml
```

At minimum, set `[server]`. Without a `config.toml` the app starts and says so
rather than looking broken.

RetroArch is found automatically in the usual places per OS. If yours lives
somewhere else — a second drive, a portable install — set it in **Settings**, or
as `[retroarch] root`. Missing cores and the slang shader pack are downloaded on
first launch of a game that needs them.

## Using it

Arrows or the left stick move, Enter or the bottom face button opens and plays,
Esc or the right face button goes back, `/` searches, `?` lists every binding.
Keyboard and controller are rebound separately in Settings; both persist.

Launching writes a temporary config that RetroArch layers on top of its own, so
**your `retroarch.cfg` is never modified**. That layer carries a consistent
gamepad hotkey set — RetroArch binds keyboard hotkeys but none for a pad, which
otherwise leaves a handheld user unable to quit a game. Hold **Back/Select**:

| with | does |
|---|---|
| A | quit (asks twice) |
| B | toggle shaders |
| X | FPS counter |
| Y | RetroArch menu, paused |
| LB / RB | load / save state |
| RT | fast-forward |
| D-pad ↑ ↓ | previous / next shader |
| D-pad ← → | previous / next save slot |

Anything in your own settings file is applied last and wins.

## Layout

```
src/            core library — api, cache, download, media, theme, retroarch
src/main.rs     CLI (clap) and TUI
src-tauri/      Tauri GUI shell, thin: it delegates to the library
ui/             static ES modules and CSS, no bundler
ui/icons/       Lucide (ISC), vendored — see ui/icons/README.md
tools/          one-shot Python for DAT analysis, BIOS sets, server sync
data/           generated reference data (core map, arcade names, catver)
```

The GUI, TUI and CLI share one launch planner. Adding an emulator quirk in one
place fixes it in all three — which is the point, because it did not used to.

## Status

Browsing, downloading and launching all work. **Save sync does not**: the API
layer exists and the scanner runs, but nothing calls it yet. That is the largest
outstanding piece.

`data/` also carries the arcade work: every romset launch-tested against FBNeo
and two MAME versions, with the results in `arcade-core-test.json`.
