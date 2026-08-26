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
