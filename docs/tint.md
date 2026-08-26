# The tint

A flat wash over the whole app on the AYN Thor, Android 13. Not fixed. This is
everything measured, and everything ruled out, so the next person starts where
this stopped rather than where it started.

Reported by Frank repeatedly from 2026-08-26. Roughly a dozen attempts, none of
which worked. One of them — a `background` on `html` added *while looking for
it* — made it materially worse for several rounds before being found by
bisection and removed.

## What it looks like

The interface is washed out and grey where it should be near-black. It is
absent for the first frames of a launch and permanent from the first press of
the d-pad or stick. Frank consistently describes it as appearing "the moment I
move the left stick or dpad".

## What it measurably is

**A contrast transform, not something drawn on top.** This is the single most
important fact and it took far too long to establish. Painting the page a known
colour and reading the device's framebuffer:

| page painted | device shows |
|---|---|
| `#FF0000` | `#EA3D31` |
| `#14161A` | `#303238` |
| `#000000` | `#1F1F1F` |

Whites are pulled down and blacks are lifted. No overlay of any single colour at
any single alpha produces that pair of results — `#FF0000 → #EA3D31` and
`#14161A → #303238` cannot both come from mixing with one colour. Every attempt
that assumed "something white is on top" was therefore doomed, and most of the
attempts below assumed exactly that.

**Canvas pixels are exempt.** The WebGL backdrop measured `#030306` in the same
frame where CSS-painted regions measured `#313236`. Whatever performs the
transform does not touch canvas output. This is also why the app looks correct
until the canvas stops covering the viewport, and why cover art looks right
while the page around it does not.

**It is not compositing.** `dumpsys SurfaceFlinger` for the app's layer:

```
blend=NONE (1) alpha=1.000000 backgroundBlurRadius=0 composition type=DEVICE (2)
isOpaque=true colorTransform=[identity] dimming enabled=true
```

Alpha is 1, blend is NONE, the colour transform is the identity matrix. The wash
is already in the raster the app hands over.

**The page does not know.** `getComputedStyle(document.documentElement)
.backgroundColor` reports `rgb(20, 22, 26)` throughout, while the device draws
`#303238`. Nothing observable from inside the page differs between the washed
and unwashed states.

## The environment

```
device            AYN Thor, Android 13 (SDK 33), arm64-v8a
WebView           com.android.webview 109.0.5414.123   (AOSP, not updatable)
                  com.google.android.webview NOT installed
                  com.android.chrome 151 installed but not offered as provider
viewport          833 x 469 CSS px, devicePixelRatio 2.30625
app targetSdk     36
night mode        on
```

`color-mix()` is unsupported at Chromium 109, which is a separate and real
problem this app works around with an `@supports` block. It is *not* the tint —
deleting that block from the live stylesheet on the device changed nothing.

## Ruled out

Each was built, installed and measured on the device. None changed the numbers
in the table above.

* The stylesheet. The page painted pure black with `!important` still washes.
* The `@supports not (color-mix(...))` fallback block, deleted live.
* The backdrop canvas — hidden, shown, removed from the DOM, and its WebGL
  context switched to `alpha: false`.
* The toast, and its missing `color-mix` background.
* `backgroundColor` on the window in `tauri.conf.json`.
* The Android theme's `windowBackground`, `colorBackground` and
  `windowSplashScreenBackground`, set to the app's colour in both `values/` and
  `values-night/`.
* `WebView.setBackgroundColor`, applied in `onWebViewCreate` and again on every
  focus change in case wry overwrites it.
* `WebSettings.forceDark = FORCE_DARK_OFF`. Reads back as `AUTO` — it is a no-op
  at targetSdk 33+.
* `WebSettings.isAlgorithmicDarkeningAllowed = false`. Reads back `false`.
* Both of the above again through `androidx.webkit`'s `WebSettingsCompat`, which
  routes to whatever the installed WebView implements.
  `WebViewFeature.isFeatureSupported` returns true for both. No effect, before
  or after a page reload.
* `<meta name="color-scheme" content="dark">`, in addition to the existing
  `color-scheme: dark` in CSS.
* Android accessibility filters. All off:
  `accessibility_display_daltonizer_enabled`, `reduce_bright_colors_activated`,
  `high_text_contrast_enabled`, `accessibility_display_inversion_enabled` are
  null or 0.

## What was found by bisection, and mattered

Removing the `mobile` classes from `html` and `body` at runtime dropped the
dominant colour from `#303238` to `#232327`. Deleting one rule did the same
alone:

```css
html.mobile.backdrop-on { background: var(--bg); }
```

That rule had been added a few rounds earlier while chasing the tint, on the
theory that a transparent page over a light window was the cause. It was the
opposite: with a backdrop running the page is *meant* to be transparent and the
canvas provides the colour, and painting `html` gave the transform a large CSS
background to act on. It is removed. The tint is smaller without it and still
present.

**Bisection should have been the first move, not the tenth.** It located a real
contributor in two steps after many rounds of hypothesis-led guessing found
nothing.

## Where to look next

1. **Establish whether it is the panel or the capture.** Everything here is
   `adb exec-out screencap`. Frank reports seeing it, but no test has separated
   "the app renders this" from "screencap reports this". Photograph the screen,
   or display a known colour full-screen from a non-WebView app and capture that
   for comparison.
2. **A trivial page in the same WebView.** Load `data:text/html,<body
   style=background:#000>` and measure. Washed means the engine or the ROM and
   nothing in this app can reach it; clean means it is this page and the search
   has been in the wrong place.
3. **HDR and colour space.** The panel is HDR (`mMaxLuminance 420`), and
   SurfaceFlinger reports `dimming enabled=true`. sRGB `#FF0000` expressed in
   Display-P3 is roughly `(234, 51, 35)`, and the measurement is `(234, 61, 49)`
   — close enough to be worth ruling in or out properly. Check the layer's
   dataspace and whether the WebView surface is wide-gamut.
4. **A different WebView.** `com.android.chrome` 151 is installed. If it can be
   made the provider — Developer options, WebView implementation — that
   distinguishes engine from ROM in one step.

## What not to repeat

Do not add a background to `html` or `body` on Android to "cover" it. That was
tried, it is what made the tint worse, and the removal is documented above.
