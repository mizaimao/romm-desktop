# Device configuration for the Miyoo Flip

Files here are applied to the handheld, not built into anything.

## `flip-hotkeys.conf`

Appended to `/userdata/system/knulli.conf`. Everything is **Hotkey** (the Menu
button) plus the named button:

| Press | Does |
|---|---|
| B | Exit RetroArch — **twice**, so it cannot happen by accident |
| X | RetroArch menu |
| Y | Toggle shader |
| A | Show FPS |
| L1 / R1 | Load state / Save state |
| L2 / R2 | Pause / Fast-forward (hold) |
| D-pad ← → | Previous / next save-state slot |
| D-pad ↑ ↓ | Previous / next shader |

Set here rather than inside RetroArch because **configgen rewrites
`retroarch.cfg` at every launch** — which is why changes made in RetroArch's own
menu did not stick. `knulli.conf` lives on the persistent partition and is
applied on top at each start.

The stock layout had **four double bindings**, and one of them was dangerous:
Hotkey+A was `reset` *and* `fps_toggle`, so there was no way to see the frame
rate without restarting the game. Y was save-state *and* shader-toggle; L2 and
R2 each did two things. The five now bound to `nul` at the bottom of the file
are the leftovers of those pairs.

## `flip-shaders.conf`

Also appended to `/userdata/system/knulli.conf`.

`sharp-shimmerless` is the shader that matters on this screen. At 640×480 none
of these systems divide evenly — SNES is 256×224, so 2× leaves 32 rows over —
and the usual fix, bilinear, smears everything. Shimmerless keeps the pixel
grid even and only softens the seam. The `-lcd-crt` set applies it everywhere
and adds an LCD grid on gb/gbc/gba/gamegear, scanlines on the consoles.

Everything in the set is one pass except the handheld preset, which is two.

## `retroarch-shaders/`

Copied to `/userdata/system/configs/retroarch/shaders/`, which is RetroArch's
`video_shader_dir`. **Hotkey + D-pad ↑/↓** cycles the presets in that
directory, so these three are the alternatives reachable in-game: plain
shimmerless, shimmerless with scanlines, and shimmerless with the LCD grid.

They spell out absolute paths because RetroArch resolves a preset's shader
paths relative to the preset, and these no longer sit beside their `.glsl`
files.

## `gba-bezel/`

Copied to `/userdata/decorations/romm/`, then selected with `gba.bezel=romm`.

KNULLI's own GBA bezel renders the wordmark in a washed-out khaki-olive
gradient. This is the same artwork and the **same geometry** — the game keeps
the full 640 width and the 84px apron at the bottom carries the label — with
the wordmark recoloured to the console's metallic silver.

It lives under `/userdata` because `/` is an overlay whose writable layer is a
256 MB tmpfs: anything written to `/usr` is gone at the next boot.
