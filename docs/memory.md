# What the app weighs, and why

> **Read this section first.** The measurements in this file contradict each
> other and none of the conclusions drawn from them are safe. What follows is
> kept so the record is legible, not because it is right. The one thing here
> that is settled is the asset-request bug, and it is settled because it
> reproduces every time.

## Settled: a picture costs by its own size, not the size it is drawn

Three earlier attempts at this contradicted each other, every time because the
layout changed between runs and so did how many pictures were actually
painted. This one holds the layout absolutely still — twenty slots of exactly
200x150, `object-fit: cover`, all of them on screen, identical in every run —
and varies only the resolution of the file behind each slot. Loaded one at a
time, held at 1, 5, 10 and 20 long enough for the sampler outside to catch
each step.

| art | file | drawn at | 0 up | 1 | 5 | 10 | 20 | per picture |
|---|---|---|---|---|---|---|---|---|
| 3dboxes | 325x600 | 200x150 | 280.2 | 328 | 324 | 343 | **331.3** | **2.6 MB** |
| miximages | 1280x960 | 200x150 | 239.4 | 263.1 | 326.2 | 361.9 | **471.1** | **10.9 MB** |

The miximages column is clean — the spread across repeated samples at each
step is 0.1 MB — and it is monotonic: 239, 263, 326, 362, 471. The slope from
ten pictures to twenty is 10.9 MB each.

**Both are drawn at exactly the same size. One costs four times the other.**
The only thing that differs is the file: 1,228,800 pixels against 195,000, a
ratio of 6.3. The cost ratio is 4.2 — not perfectly linear, because there is a
fixed overhead per picture, but the direction is not in doubt.

So the claim made earlier in this file — that WebKit already decodes at the
drawn size, the way Coil does with `inSampleSize` and QML with `sourceSize` —
is **wrong**. It decodes at the file's size. 10.9 MB for a 1,280x960 picture
is about 2.2x a plain RGBA decode of it, so there is a compositing or GPU copy
on top of the decode as well.

Frank's instinct throughout — that the size of the artwork must matter — was
right, and three of my measurements said otherwise because all three were
measuring the layout instead.

### What this makes possible

Ninety cards of miximage artwork is around 980 MB by this measurement, which
is the arcade screen, and it is the whole problem.

The fix does not touch a single file on disk. Fetch the picture as a blob,
`createImageBitmap` it, draw it into a `<canvas>` the size of the tile, and
`close()` the bitmap. What is *retained* is then the canvas — 200x150 at 2x is
0.24 MB — instead of the decoded file. The full-resolution original is still
what the detail pane opens, still exactly the bytes that were downloaded, and
still the only copy that exists.

Twenty pictures would go from 218 MB to about 5 MB. The peak becomes one
decode at a time rather than ninety held at once.

Safari has historically ignored `createImageBitmap`'s `resizeWidth` options,
so the intermediate bitmap may well be full size — but it is transient and
closed immediately, and only one exists at a time. That is the difference
between a 4.9 MB spike and a 980 MB resident set.

## The baseline, at last: 235.6 MB with nothing of ours on the screen

Asked for repeatedly and put off for too long while I chased the artwork.
Measured with a full-screen opaque overlay, so no card, no cover and no
backdrop is being painted, and with the per-process split kept for every
sample:

| | as loaded | fully covered |
|---|---|---|
| romm-gui (Rust: cache, API client, Tauri) | 48.7 MB | 48.7 MB |
| WebKit GPU | 27.6 MB | 27.6 MB |
| WebKit Networking | 6.2 MB | 6.2 MB |
| WebKit WebContent (the page) | 153.1 MB | 153.0 MB |
| **total** | **235.6 MB** | **235.5 MB** |

Covering everything changes nothing — a tenth of a megabyte. So on the console
screen the floor is structural, not painted: it is the runtime, the page, the
row data and WebKit's own baseline, and hiding things does not touch it.

Two thirds of it is WebContent. A blank WKWebView is usually somewhere around
60–90 MB before a page is loaded, so our page is plausibly another 60–90 on
top — but that split has *not* been measured and should not be quoted as if it
had.

This is the number every optimisation gets measured against.

## A burst of asset requests wedges the page

Found on the way. Sixty `<img>` pointed at `asset://` URLs *at once* never
load and never fail: no `onload`, no `onerror`, nothing in two minutes, and
the process sits perfectly still. The same sixty loaded one after another all
succeed, every time.

`flushCovers` sets up to forty `src`s in a single pass. That is very likely
the same burst, and very likely why three attempts to open the arcade console
from a script froze the page — and worth holding against "it feels slow and
laggy when browsing platform games", which has been an open complaint since
`docs/parked.md` §14 and was never explained.

This is a real bug and it outranks anything about memory.


Measured on 2026-08-22, on Frank's Mac, against his own library — 24 consoles,
2,504 arcade games, `list_art = "miximages"`. Numbers are macOS *physical
footprint*, which is what Activity Monitor's Memory column shows; RSS
overcounts shared libraries badly and is not comparable between the two front
ends.

Repeat any of this with `ROMM_MEASURE=tools/browse.js` (Tauri) or
`ROMM_SDL_SHOT` / `ROMM_SDL_FRAMES` (SDL). Neither needs anyone watching.

## The numbers

| | Tauri | SDL |
|---|---|---|
| Fresh launch, consoles screen | **228 MB** | **227 MB** |
| Arcade grid open, scrolled to the bottom | **peak 1,030 MB**, settling to 550 | not measured |

The two front ends start out level. The whole difference is what happens when
a wall of artwork goes past.

Tauri's 228 MB splits as romm-gui 33, WebKit GPU 29, Networking 6, WebContent
160. After browsing, WebContent alone is 447 MB and the peak was over 850.

## The A/B that changes the reading

Same script, same library, only `list_art` different — so only the size of the
artwork changed:

| `list_art` | decoded, each | peak | settles at |
|---|---|---|---|
| `3dboxes` (325x600) | 0.78 MB | 691 MB | **436 MB** |
| `miximages` (1280x960) | 4.9 MB | 1,030 MB | **550 MB** |

Six times the artwork did not cost six times the memory. It cost 1.26x.

Work back from the settled figures, over the 228 MB consoles-screen floor:

* 3dboxes: 208 MB of images at 0.78 MB each = about 266 pictures held.
* miximages: 322 MB of images at 4.9 MB each = about 66 pictures held.

Sixty-six against two hundred and sixty-six. **WebKit is already bounding its
decode cache by bytes rather than by count** — the same shape as ES-DE's
`MaxVRAM`, arrived at independently, and it holds a few hundred megabytes of
pictures whatever size they happen to be.

That is the opposite of what the first measurement suggested, and it matters:
there is no unbounded leak in the webview to go and fix. What is left is a
budget somebody else chose, sized for a Mac with plenty of memory, and a
**peak** — 691 MB, 1,030 MB — reached before the pruning catches up.

The peak is the number that matters on Android. A steady state of 550 MB on a
6 GB device is uninteresting; a transient gigabyte inside a single-process
WebView on a 2 GB device is a kill.

## Why the pictures cost what they do


The artwork is 1,280×960. Decoded that is 1,280 × 960 × 4 = **4.9 MB per
picture**, and the grid draws them 150 points wide. Every cover on screen
costs about ten times what it needs to.

That single number explains everything else:

* 447 MB of WebContent ÷ 4.9 MB ≈ 91 covers held. It is not WebKit being fat
  in general. It is ninety-one full-size pictures inside a budget that would
  have held four hundred small ones.
* `src-sdl/src/covers.rs` caps the cache at 192 *textures* and its comment
  works that out as 150 MB, on the assumption that a cover is 786 KB. At
  4.9 MB the same cap is **940 MB**. The bound is wrong, and it is wrong in
  the dangerous direction — the SDL front end only looks thrifty because
  nothing has yet scrolled far enough to reach it.

A count is not a budget. Both caches need to count bytes.

## How ES-DE gets away with it

It is the obvious comparison: ES-DE runs on Frank's MagicX Mini Zero 28, which
has 2 GB, with the same full-size artwork, and it is fine. It does not
downscale — `TextureData.cpp` loads at native size and uploads BGRA8 straight
to the GPU, four bytes a pixel, exactly like ours.

What it has is a byte budget with eviction. From `TextureDataManager.cpp`:

```cpp
size_t settingVRAM {static_cast<size_t>(Settings::getInstance()->getInt("MaxVRAM"))};
size_t max_texture {settingVRAM * 1024 * 1024};
...
for (auto it = mTextures.crbegin(); it != mTextures.crend(); ++it) {
    if (size < max_texture)
        break;
    (*it)->releaseVRAM();
    (*it)->releaseRAM();
    mLoader->remove(*it);
    size = TextureResource::getTotalMemUsage();
}
```

Least-recently-used first, and it drops *both* copies — the GPU texture and
the decoded bytes in RAM. `MaxVRAM` is a user setting, clamped to 128–2048 MiB.

So the answer to "how do they show full-res artwork on 2 GB" is: they hold
however many full-res pictures fit in a fixed number of megabytes, and throw
the rest away. Sixty-four full-size covers is 313 MB and is far more than is
ever on screen at once. Nothing is resampled and nothing is lost.

## What that means for us

**SDL:** the fix is small — make `covers.rs` count bytes instead of textures,
with a budget rather than a count, and pick the budget from the machine.

**Tauri:** we cannot set the budget — WebKit's decoded-image cache is not ours
and there is no API for "hold at most 200 MB of images". But the A/B above
says it already has one. What we can change is what goes into it, and the
peak, which is where the danger is.

The lever we do have is the asset protocol: the page asks for a picture over
a URL we serve, so the *served* bytes can be a resized copy while the file on
disk stays exactly as downloaded. That is the same trade Android's Coil and
Glide make with `inSampleSize` — decode at the size you are going to draw —
and it is invisible to the source. The detail pane, which draws big, keeps
asking for the original.

Halving the served size quarters the bytes, which both lowers the peak and
lets four times as many covers sit inside whatever budget the platform has
chosen. It is the only lever that works on a cache we do not own.

This matters most on Android, where the WebView is single-process: it runs
*inside* our app, against the per-app limit, with no separate WebContent
process to absorb the spike. A 1,030 MB peak is not survivable on a 2 GB
device. Chromium also decodes the whole image before resampling it down, so
the intrinsic size is what costs, not the drawn size.


## What is measured and what is not

Measured: everything in the tables above, with the script's own notes
confirming the browse actually happened. `tools/browse.js` reports through the
`measure_note` command because the page's `console.log` goes to the webview
console and nowhere a shell can read it — one run looked flat and was in fact
a browse that never started.

Not measured, and worth doing before anything is built on this:

* **Android.** Every number here is macOS and WKWebView. Chromium sizes its
  decode cache from device memory and decodes differently. The 2 GB device is
  the question and nothing here answers it.
* **The peak.** Sampled every four seconds, so the true peak is higher than
  1,030 MB. It wants a tighter sampler, or a run under `instruments`.
* **The floor.** 228 MB on the consoles screen is itself unexplained — 24
  console pictures and a strip of covers should not cost that. Worth taking
  apart before optimising anything downstream of it.
* Opening the arcade console by script hung twice with `list_art =
  "miximages"` and worked with `3dboxes`. Frank browses it daily without
  trouble, so this is most likely the script driving the page in a way a
  person does not. It has not been explained, and the miximages row above came
  from a session-restore rather than a confirmed scripted browse.


## Correction, 2026-08-22: the measurements below are not trustworthy

Frank, on being told that showing fewer pictures cost the same memory:

> I strongly disaggree with your testing results. Showing fewer images eat the
> same amount of ram this is mathmatically impossible.

He is right, and the claim is withdrawn. It rested on one pair of numbers —
436 MB before the margin change, 439 MB after — and **the image count was
never measured**, so it was never established that fewer pictures were on the
page at all. A number that does not move is equally consistent with a cache
that will not let go and with a change that never took effect, and those want
opposite fixes. Asserting the first one without checking was the error.

Three attempts to check it since have all failed, and they failed in ways that
say the rig is wrong rather than the app:

* Opening the arcade console from a script freezes the page. Twice through
  `showRoms`, once through a synthetic click on the card. No error, no further
  notes, memory flat. Frank does it by hand daily without trouble.
* A direct experiment — sixty full-size miximages put on screen through the
  asset protocol, weighed, removed, weighed again — never got past putting
  them up. Neither `onload` nor `onerror` fired for any of the sixty in two
  minutes, and the footprint never moved off 228 MB. Assets requested this way
  hang rather than fail, which is worth understanding on its own.

So the only figure below that came from a confirmed browse is the `3dboxes`
row. The `miximages` row came from a session restore. Everything derived from
comparing them — including "WebKit already has ES-DE's budget" — is a
hypothesis with one leg to stand on, not a finding.

**Nothing should be built on this until the rig is fixed.** What the rig needs:
a browse that is confirmed to have happened, a count of the pictures on the
page beside every weight, and a sampler faster than four seconds.

## Tightening the margins did nothing — unverified

Frank's idea, on 2026-08-22:

> even at 1080P or 1440P the screen is capped at 7 or 6 inches. That means not
> many tiles can be displayed. Can we do optimizations that we only load
> images that are on the display and off load the ones that do not so that we
> effectively reduce the total image size to less than 12 or soemthing?

The release margin was a flat 1,600px, which is two screens on the window it
was written against and *eight* on a 720-tall handheld — so the machine with
the least memory hoarded the most. That is worth fixing whatever else is true,
and it is fixed: both margins are now a fraction of the list's own height,
0.4 screens to load and 1.0 to release.

It moved the number by three megabytes. 436 MB before, 439 after — and see the
correction above: nothing counted the pictures, so this does not show what it
was claimed to show. The margin change is right regardless, because eight
screenfuls of hoarding on a handheld is wrong whatever it costs.

The hypothesis it suggested — that releasing the `<img>` does not release the
decode, because WebKit's image cache is keyed by URL and outlives the element —
is a real and documented behaviour, but it has not been demonstrated *here*.
It is the first thing the fixed rig should settle.

If it turns out to be true, two things work:

1. **Serve fewer bytes.** Resize in the asset protocol, source untouched. A
   500 KB cover instead of a 4.9 MB one means the same budget holds ten times
   as many, and the peak — the number that kills an app on Android — falls
   with it.

2. **Own the decode.** `fetch` the file as a blob, `createImageBitmap` it,
   draw it into a canvas the size of the tile, and `close()` the bitmap. The
   URL never goes through an `<img>`, so WebKit's image cache never gets an
   entry, and what is retained is a canvas backing store — 300x420, half a
   megabyte — that we can throw away when we say. This is the honest port of
   ES-DE's budget into a webview, and it is the only way to actually get to
   Frank's "twelve pictures".

Both are worth doing and (1) is most of the win for a tenth of the work.


## What the open-source Android frontends actually do

Asked for on 2026-08-22, because "praised on a 2 GB device" and "brute-force
the RAM" cannot both be true. They are not doing anything exotic. They are
doing four ordinary things, and **not one of them touches the source file.**

**1. Decode at the size you are going to draw.** Universal, and the big one.
Coil and Glide do it through `inSampleSize` — read the header, work out the
factor, decode at a fraction (Lemuroid and Emulair are both Coil). Pegasus does
it through QML's `sourceSize`, which Qt's own documentation is blunt about:
images are usually the greatest user of memory in a QML interface, and anything
not part of the interface should have its size bounded this way. Flutter's
`ResizeImage` is the same idea for Yuno.

This is not reducing anyone's artwork. The file stays 1280x960. What changes is
that the decoder is not asked to produce a million pixels for a tile that can
show fifty thousand. The full file is still what the detail pane opens.

**2. A memory cache measured in bytes, with LRU eviction.** Coil's default is
`maxSizePercent(context, 0.25)` — a quarter of what the device has, so it is
80 MB on a 2 GB handheld and 800 MB on a desktop, with no code change.
ES-DE's `MaxVRAM` is the same idea with a manual slider, 128–2048 MiB, and its
eviction drops the GPU texture *and* the decoded bytes.

**3. A disk cache of the decoded result**, so the resize happens once ever
rather than once per scroll. Coil's is `DiskCache.maxSizePercent(0.02)`. This
is exactly Frank's own suggestion — build a thumbnail cache at first launch,
keep the full-resolution files intact — arrived at independently.

**4. Two things that only exist on Android**, and are most of why a 2 GB phone
copes at all:

* `Bitmap.Config.RGB_565` — half the bytes of `ARGB_8888` at the *same*
  resolution, for artwork with no transparency. Glide used it by default for
  years.
* **Hardware bitmaps** (`Bitmap.Config.HARDWARE`, Coil's default on API 28+).
  The decoded image lives in graphics memory rather than the app's heap, so it
  does not count against the per-app limit at all.

A webview has (1) — `createImageBitmap` into a tile-sized canvas — and can be
given (2) and (3). It has no equivalent of (4), and that is a real structural
disadvantage of shipping a webview to Android rather than a native view.

The fallback Frank named is also what the field does: ES-DE's list view, and
most others' default, show one piece of artwork for the selected game and
nothing for the rest. One picture on screen is one picture in memory.
