# What everyone else builds these frontends with

Asked on 2026-08-24, after the measurement in `memory-footprint.md` put 106 MB of our
192 MB floor inside WebKit and none of it inside our own interface.

## Nobody uses a webview

| Frontend | Stack | Platforms | Source |
|---|---|---|---|
| ES-DE | C++, SDL2, OpenGL ES, own renderer | Win / macOS / Linux / Android | available |
| Pegasus | C++, Qt QML | Win / macOS / Linux / Android | open |
| Yuno | Flutter, Dart | Win / macOS / Linux / Android | open |
| Lemuroid | Kotlin, RecyclerView, Coil | Android only | open |
| Daijishō | native Android, Kotlin/Java | Android only | closed |
| Dawn (MagicX) | same lineage as Daijishō | Android only | closed |
| Beacon, Dig, Reset Collection | native Android | Android only | closed |

Not one of them ships a browser engine. That is the whole of the difference:
their floor is a widget toolkit, ours is WebKit.

## What could replace it, keeping one codebase

Only the first three keep Windows, macOS, Linux and Android in one tree, which
is the requirement.

**Slint.** Rust first, which matters more here than anywhere else — the entire
backend is already Rust and would not be touched. Desktop, Android, iOS, web
and microcontrollers from one UI file; the runtime is under 300 KiB. The UI is
declarative markup with real widgets, layout, text and images, so it is not the
SDL bargain of hand-placing rectangles. Android support was funded through NGI
Zero Core and landed in 2025.

**Flutter.** What Yuno uses. Excellent interfaces, every platform, mature. But
Dart, so the app is written twice in two languages — the Rust core would live
behind `flutter_rust_bridge` and every command crosses it.

**Qt / QML.** What Pegasus uses, and it is proven on exactly this problem —
`sourceSize` and proper list recycling are built in. C++, a large dependency,
and licensing to think about.

**SDL2** is the fourth option and it is already tried: `sdl-port`, parked in
`parked.md`. It reaches every platform and costs the entire interface, drawn by
hand. Frank's own verdict on how far that got in a week was "day and night,
maybe two generations".

## What a migration would actually buy

The 106 MB WebKit process and the 29 MB GPU process go, and the remaining two
merge into one. A realistic floor is 50–80 MB against today's 192.

What it would *not* buy: the 49 MB our own Rust process uses. That is ours, it
is the one number no framework is imposing, and it comes along whichever way we
go.

## Against the four things the interface actually needs

Frank's constraints, 2026-08-24: frosted glass over a moving backdrop, glow,
layout that reads like HTML and CSS, and the shader backdrop. That changes the
ranking, because most toolkits do not have a backdrop blur at all.

| | glass over a backdrop | glow | layout | shader backdrop | Rust core | floor |
|---|---|---|---|---|---|---|
| **Tauri, today** | `backdrop-filter`, one line | `box-shadow`, one line | it *is* CSS | WebGL canvas | untouched | 192 MB |
| **Flutter** | `BackdropFilter`, plus GLSL through `FragmentProgram` | `BoxShadow` | Flex/Row/Column, close in spirit | `FragmentProgram` | behind a bridge | 40–80 MB |
| **Qt / QML** | `MultiEffect` blur | `MultiEffect` glow | anchors and Layouts, less CSS-like | `ShaderEffect` | behind C++ | 40–70 MB |
| **Slint** | **none** — [slint#2066](https://github.com/slint-ui/slint/issues/2066) is still open | drop shadow only | flexbox-ish | limited | native | 30–50 MB |
| **Cocoon** (Kotlin + Jetpack Compose + Coil 3, Android only) | `Modifier.blur` / Haze | `shadow` | Compose modifiers | AGSL | would be a rewrite | — |
| **SDL2** (parked) | we wrote it | we wrote it | we wrote the layout engine | we wrote it | native | 30–60 MB |

**Slint is out.** The one thing it cannot do is the thing the whole look is
built on: blurring what is behind a rectangle is an open feature request, not a
feature. That reverses the recommendation made higher up this page before the
constraints were known.

That leaves Flutter and Qt as the only migrations that keep the look, and
**Tauri as the best fit for these four constraints by some distance** — three
of them are one line of CSS each, and the fourth already works.

## What Tauri actually ships

Worth being exact, because it changes the judgement. **Tauri does not bundle a
browser.** That is Electron, which ships all of Chromium in every app. Tauri
uses the web view the operating system already has — WKWebView on macOS,
WebView2 on Windows, WebKitGTK on Linux, the system WebView on Android — so
the binary is small and nothing is duplicated on disk.

What it costs is not disk, it is the 106 MB that WebKit process occupies at
run time. Shared code, private memory. So the trade is real but it is not
"we ship a browser with the app".

## The recommendation

**Not yet, and fix the 49 MB first.** It is free, it helps under every option,
and until it is understood we do not know what a migrated app would weigh
either.

After that it is a judgement about devices, not about elegance: 192 MB is
nothing on a 6 or 16 GB handheld and is a real constraint on the 2 GB one that
Frank rarely uses.

And with the four constraints on the table, staying is the stronger case than
it looked an hour ago. Frosted glass, glow and the layout are one line of CSS
each; in every alternative they are a widget, an effect node or a shader we
maintain. If the 2 GB device ever stops being hypothetical the answer is
**Flutter** — the only one that keeps the whole look and reaches every platform
— and the price is the app being written in two languages with the Rust core
behind a bridge.
