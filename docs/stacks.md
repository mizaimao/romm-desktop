# What everyone else builds these frontends with

Asked on 2026-08-24, after the measurement in `memory.md` put 106 MB of our
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

## The recommendation

**Not yet, and fix the 49 MB first.** It is free, it helps under every option,
and until it is understood we do not know what a migrated app would weigh
either.

After that it is a judgement about devices, not about elegance: 192 MB is
nothing on a 6 or 16 GB handheld and is a real constraint on the 2 GB one that
Frank rarely uses. If the 2 GB device stops being hypothetical, the answer is
**Slint** rather than SDL — same reach, same Rust, and the UI is written as
markup instead of drawn as rectangles, which is precisely the gap that made the
SDL branch look two generations behind.
