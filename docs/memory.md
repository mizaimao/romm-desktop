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

## Why

The artwork is 1,280×960. Decoded that is 1,280 × 960 × 4 = **4.9 MB per
picture**, and the grid draws them 150 points wide. Every cover on screen
costs about ten times what it needs to.

That single number explains everything else:

* 447 MB of WebContent ÷ 4.9 MB ≈ 91 covers held. It is not WebKit being fat.
  It is ninety-one full-size pictures.
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

**Tauri:** we cannot do this. WebKit's decoded-image cache is not ours; there
is no API for "hold at most 200 MB of images", and releasing the `<img>` (which
`observeCovers` already does) does not release the decode. The 1,030 MB peak
is WebKit deciding on its own when to prune, and it prunes late.

The lever we do have is the asset protocol: the page asks for a picture over
a URL we serve, so the *served* bytes can be a resized copy while the file on
disk stays exactly as downloaded. That is the same trade Android's Coil and
Glide make with `inSampleSize` — decode at the size you are going to draw —
and it is invisible to the source. The detail pane, which draws big, keeps
asking for the original.

This matters most on Android, where the WebView is single-process: it runs
*inside* our app, against the per-app limit, with no separate WebContent
process to absorb the spike. A 1,030 MB peak is not survivable on a 2 GB
device. Chromium also decodes the whole image before resampling it down, so
the intrinsic size is what costs, not the drawn size.
