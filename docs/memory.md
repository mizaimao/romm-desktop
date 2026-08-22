# What the app weighs, and why

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


## Tightening the margins did nothing, and that is the finding

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

It moved the number by three megabytes. 436 MB before, 439 after.

Which says the thing worth writing down: **releasing the `<img>` does not
release the decode.** WebKit's image cache is keyed by URL and outlives the
element. `observeCovers` has been putting the placeholder back for weeks and
the pictures never went anywhere. The count of cards on screen is not the
lever; the cache's own byte budget is, and it is not ours.

So there are exactly two things that work:

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
