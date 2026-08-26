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

mugwomp93's Perfect GBA overlay, from
[ourigen/perfect_overlays](https://github.com/ourigen/perfect_overlays) —
drawn for 640×480 handhelds, which is exactly this screen, so nothing is
stretched. "GAME BOY" in silver, "ADVANCE" in the rainbow gradient.

It is a full-screen RetroArch overlay rather than a frame with a hole in it:
every pixel carries some alpha, ~60–71 over the picture, which is the LCD
grid. The `.info` says the game window is the full 640 wide and the top 426
rows; the bottom 54 are the opaque label. That is 640×426 ≈ 3:2, which is
GBA's native aspect, so it lands on whole pixels.

Batocera bezels are RetroArch overlays underneath, which is why this drops in.

Because it draws its own grid, `gba.shaderset` is set to plain
`sharp-shimmerless` — the `-lcd-crt` set would put a second grid on top.

## `hotkey/`

`multimedia_keys.append` is appended to a copy of KNULLI's own
`multimedia_keys.conf`, saved as `/userdata/system/configs/multimedia_keys.conf`.
`S50triggerhappy` prefers that path over anything in `/etc` — which matters,
because `/etc` is on the tmpfs overlay and would be gone at the next boot.

**L2+R2** runs `romm-launch.sh`. It has to be a two-button combo whose trigger
is the *second* button: triggerhappy matches on the exact set of held buttons,
so a rule on Menu alone would fire the instant Menu went down, before the
second button of any Menu combo.

`romm-launch.sh` exits immediately if an emulator is running, because
triggerhappy sits below RetroArch and L2/R2 are ordinary game buttons.

ES's own L2/R2 navigation is turned off by copying
`/usr/share/emulationstation/es_input.cfg` to
`/userdata/system/configs/emulationstation/` with the Flip's `l2` and `r2`
entries dropped. Every other pad it ships with comes across untouched.
