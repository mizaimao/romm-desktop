# A handheld front end

Three pieces of work, in order. The first two are worth doing on their own; the
third is the one that needs a handheld to test against.

Written to be picked up cold. Every number here was measured on 2026-08-20, and
the command that produced it is given so it can be re-checked rather than
trusted.

## Why this exists

`PLAN.md` §4 chose Rust + Tauri, and the reasoning was sound:

> **UI feel.** Tauri renders in the system webview … so the look is fully
> controllable via HTML/CSS. Flutter paints its own widgets and reads slightly
> off on desktop.
> **Packaging.** ~10 MB bundles with a documented macOS signing/notarization path.

Read what is absent: any mention of a handheld. The alternatives weighed were
Flutter and PySide6, the criteria were desktop feel and macOS notarization, and
the prototype was TUI-only. SDL2 was never a candidate because the requirement
that makes it necessary did not exist yet.

That requirement now exists. ArkOS/dArkOS-lineage firmware runs its frontend
directly on DRM/KMS with SDL2 and **no display server at all** — not X11, not
Wayland. Tauri needs `webkit2gtk-4.1` + GTK3, which needs one of them. So it is
not that Tauri is too heavy for the device; it is that there is nothing for it
to draw into.

**The target is RK3566** (MiniLoong Pocket 1 class): quad Cortex-A55 @ 1.8 GHz,
Mali-G52 2EE, GLES 3.2, 1 GB RAM, 4" 960x720. Allwinner A33 / Mali-400 is
explicitly **not** a target — it is GLES 2.0 only and the shaders would need
downgrading to GLSL ES 1.00 with `mediump` fragment precision.

## What is already known, so nobody measures it twice

**The split.** `src/` is 23,970 lines and entirely portable. `src-tauri/` is
3,763 lines and is throwaway. `ui/js` is 9,564 lines, of which far less is
rendering than it looks.

**Logic hiding in the front end** — `grep -cE 'document\.|querySelector|createElement|classList|innerHTML|style\.|appendChild|getElementById'`:

| Module | Lines | DOM-touching |
|---|--:|--:|
| `ui/js/bindings.js` | 251 | 0 |
| `ui/js/picker-order.js` | 120 | 2 |
| `ui/js/filter.js` | 140 | 4 |
| `ui/js/sort.js` | 157 | 10 |
| `ui/js/pagefilter.js` | 115 | 10 |
| `ui/js/gamepad.js` | 518 | 10 |
| `ui/js/keys.js` | 441 | 14 |
| **total** | **1,742** | **50** |

**Memory.** Measured with `footprint -p <pid>` on a running build, summed over
the app and the three WebKit XPC helpers that start in the same second:

| Process | Footprint |
|---|--:|
| `romm-gui` | 44 MB |
| WebKit GPU | 43 MB |
| WebKit Networking | 6 MB |
| WebKit **WebContent** | **578 MB** |
| total | **~671 MB** |

Note `ps` on `romm-gui` alone reports ~44 MB and a naive `grep WebKit` sweeps up
every other app on the machine. Neither is the answer.

**What ports, and what does not.** This is the surprising part:

* **`ui/js/backdrop.js` ports nearly as-is.** It is not CSS — it is WebGL2 with
  GLSL fragment shaders, a full-screen quad, and uniforms `u_size`, `u_time`,
  `u_low/u_mid/u_high`, `u_strength`, `u_speed`. SDL2 gives an OpenGL ES context
  directly; on Mali-G52 (GLES 3.2) the shading language maps 1:1.
* **`ui/js/tint.js` ports and gets simpler.** It averages a cover down to one
  colour for the selection glow by drawing into an 8x8 canvas. In Rust the
  images are already decoded; the canvas round-trip disappears.
* **CSS effects become draw calls.** `ui/style.css` and `ui/settings.css` are
  3,339 lines with 131 uses of `backdrop-filter`, `blur()`, `border-radius`,
  gradients and transitions. `box-shadow` glow becomes a blurred quad or a
  pre-blurred nine-slice tinted by `tint.js`'s colour. `backdrop-filter` is the
  expensive one — it samples what is behind the element, so it needs
  render-to-texture plus a blur pass.
* **Video and PDF do not port.** `ui/js/lightbox.js` plays the gameplay video and
  pauses it with space; `detail.js` and `bulk.js` handle manuals. A webview gives
  `<video>` and PDF rendering free. SDL gives neither — that is ffmpeg or libmpv,
  and it is real work. On a handheld these are reasonable to drop.
* **Text is the quiet cost.** HTML wraps, ellipsises and falls back across fonts
  unasked. SDL_ttf draws a string at a position. This library has Japanese and
  translated sets, so font fallback is not optional. It is the least glamorous
  item here and the one most likely to make the result feel cheap.

## Task 1 — move the logic into `src/`

**Do this first, and do it whether or not the handheld front end ever happens.**

Move the 1,742 lines above into the core: sort orders, filter predicates,
per-list order, page filtering, and key/pad binding resolution. The front end
then calls into the core instead of reimplementing it.

`README.md` already states the principle for the launch planner — *"The GUI, TUI
and CLI share one launch planner. Adding an emulator quirk in one place fixes it
in all three."* This is the same class of thing and it is currently not shared.

Three returns: the TUI stops being second-class today, duplication goes, and an
SDL front end inherits all of it for nothing.

**How to know it is right:** `ui/test/` holds ~300 jsdom tests asserting
behaviour rather than markup, several written after a bug with a comment saying
what they caught. Port the assertions alongside the logic. Both suites must stay
green: `npm test` and `cargo test --workspace`, plus
`cargo clippy --workspace --all-targets -- -D warnings`.

This is narrow, well-specified, test-backed work — the shape a smaller local
model handles well. One module at a time.

## Task 2 — window the middle column

Already `docs/parked.md` item #1, described there as *"the only thing in the app
that is slow rather than missing"*: 2,506 rows are inserted on every platform
switch, and the cursor, remembered scroll position, pad navigation and the lazy
cover observer all assume every row exists.

**It is also where the 578 MB is.** The observer hygiene in
`ui/js/library.js` is correct — `disconnect()` on rebuild, `unobserve()` once
loaded — but nothing ever releases a decoded image. Every cover scrolled past
stays in memory, and a 512x384 image is ~786 KB as an RGBA bitmap however small
the PNG was.

**Verify before fixing:** restart the app, `footprint` the WebContent process
before touching anything, scroll the full arcade list, measure again. If it
climbs and does not come back, that is it.

Two fixes, and they are the same fix. Windowing removes off-screen rows, and
removing the row removes its image. The cheap stopgap is clearing `img.src` when
a row scrolls well out of view, against the existing observer.

**671 MB is not inherent to Tauri.** The webview costs ~90 MB across GPU and
networking; the rest is this app holding every cover it has ever drawn.

## Task 3 — the SDL2 front end

**Add a fourth front end. Do not convert.** The repo is already CLI + TUI + Tauri
over one core. Converting would cost the desktop app its video, manuals and
glass in exchange for nothing — a Mac has a display server and plenty of RAM.
Tauri stays the desktop app; SDL targets RK3566 only.

**It stays Rust.** SDL2 is a C library; the `sdl2` crate binds to it. No C++.

**Scope:** a list, a grid, a detail pane, a few dialogs, and the two-layout
switch. `ui/js/shell.js` is only 269 lines because the architecture is already
right — views *describe* what they need and hand content back by role
(`picker`, `games`, `aside`) rather than reaching for elements. That design
ports; only the paint calls change. Estimate 2,000–2,500 lines of Rust.

**Build strategy.** Prototype windowed on macOS first — SDL2 runs the same source
there, so the loop stays at desktop speed until integration. NextUI ships a
`Tools/desktop` build for exactly this reason. On device, SDL picks `KMSDRM` at
runtime; the same binary source covers Mac, desktop Linux and handheld.

**Cross-compiling is the only real friction**, because SDL2 is C and needs
headers and libs for the target. Cheapest first: build on the device (dArkOS is
Debian, `apt install libsdl2-dev build-essential`); then `cross`; then the
`sdl2` crate's bundled feature, which builds SDL from source and links it
statically and sidesteps version drift between build box and device.

**Do not port the backdrop last.** It is the most portable thing in the front end
and getting it up early proves the GL context works on the device.

## What not to do

* Do not add X11 or a Wayland compositor to the handheld to run Tauri. It is
  possible on Debian — `cage`, or bare Xorg — and it means carrying a browser
  engine on a 1 GB device to avoid rewriting a UI layer.
* Do not treat SDL as a memory fix for the desktop app. Task 2 is that fix.
* Do not target Mali-400 / A33 without accepting a GLSL ES 1.00 shader rewrite.

## Open questions

* Font fallback strategy for Japanese and translated titles under SDL_ttf.
* Whether the animated backdrop should default off on battery — there is already
  a strength slider and per-shape settings to hang that off.
* Whether video is worth libmpv on the handheld, or simply absent there.
