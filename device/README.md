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

Choosing bezels per system, from a GUI, is parked — see `docs/parked.md`.

## `hotkey/`

`multimedia_keys.append` is appended to a copy of KNULLI's own
`multimedia_keys.conf`, saved as `/userdata/system/configs/multimedia_keys.conf`.
`S50triggerhappy` prefers that path over anything in `/etc` — which matters,
because `/etc` is on the tmpfs overlay and would be back to stock at the next
boot. `scripts/knulli.sh install` does all of this.

**L2+R2** runs `scripts/romm-hotkey.sh`. It has to be a two-button combo whose
trigger is the *second* button: triggerhappy matches on the exact set of held
buttons, so a rule on Menu alone would fire the instant Menu went down, before
the second button of any Menu combo was pressed.

The chain is `romm-hotkey.sh` → `romm-launch.sh` → `romm-sdl`. The hotkey
script is the part that deals with not having been launched by
EmulationStation: it bails out if a game is running, stops ES to get the
display — ES holds DRM master and only drops it for emulators *it* starts —
and restores ES from a trap, so a crash in the app cannot leave the device on
a black screen. ES is restarted through its init script rather than by hand,
because started by hand it comes up without `XDG_RUNTIME_DIR` and has no sound.

ES's own L2/R2 navigation is turned off by copying
`/usr/share/emulationstation/es_input.cfg` to
`/userdata/system/configs/emulationstation/` with the Flip's `l2` and `r2`
entries dropped. Every other pad it ships with comes across untouched.

## `splash/`

This is the fix for the KNULLI beetle that appeared on **every game launch and
every exit**.

It is not a setting, and it is not RetroArch or the framebuffer — both of which
I chased first and both of which were wrong. EmulationStation draws
`/usr/share/emulationstation/resources/logo.png`, a 1280×720 image of the
beetle with KNULLI under it, whenever it is loading. Launching a game and
returning from one are exactly when that happens. There is no option for it:
the file is referenced once in the ES binary and drawn unconditionally.

So `blank-logo.png` — 1280×720, fully transparent — is copied over it.

`boot-custom.sh` goes to `/boot/boot-custom.sh`, which runs as `S00bootcustom`.
It has to be that early: `/usr` is on the tmpfs overlay and is stock again at
every boot, and ES starts at `S31`, so the swap must happen before then. The
same hook also re-applies the chosen Mali driver; the GPU half was there first
and its early `exit 0`s would have skipped anything appended after it, so both
are functions now.

Reverting is deleting the hook — the real logo is on the read-only squashfs and
comes back by itself.

`custom.sh` is appended to `/userdata/system/custom.sh`, run by
`S99userservices`. It zeroes `/dev/fb0`, where `S03system-splash` leaves the
boot logo. That is **not** what was causing the flash — the framebuffer was
verified black across all three buffers while the logo was still appearing —
but it is what shows if anything stops drawing, so it is worth keeping and the
app's launcher no longer has to do it itself.
