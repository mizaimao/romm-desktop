# The addon

Frank, 2026-08-26: "SDL app will not be our romm anymore, it's going to be an
'addon' thing to vanilla Kunulli that would survive upgrades and we add manual
patches to it, inclduing our hotkeys, bazels and sync and shit so that's the
scope. Two SDL apps one was romm, archived, and now we need a new patcher one."

So there are two, and only one of them is being built:

- `src-sdl` — the RomM front end. Archived twice, see `docs/parked.md`. It is
  still in the workspace and still cross-compiles; it is not the thing below.
- the addon — new. A patcher for a stock KNULLI install, with an SDL interface
  because that is the only kind this device can show outside EmulationStation.

## Why a patcher and not a fork

KNULLI updates land as a new image. Everything outside `/userdata` is replaced,
and that is not a detail — it is most of the system:

    /            overlay, writable layer is a 256 MB tmpfs
    /overlay/base squashfs, read-only, replaced wholesale on upgrade
    /boot        vfat, mounted read-only
    /userdata    exFAT, 155 GB, the only thing that survives

Every change made this session had to be placed with that in mind, and each
one is a worked example of where a patch has to live:

| Patch | Where it survives | Why not the obvious place |
|---|---|---|
| RetroArch hotkeys | `knulli.conf` | configgen rewrites `retroarch.cfg` at every launch, so RetroArch's own menu will not hold a change |
| Shader set, per system | `knulli.conf` | same |
| Shader cycling list | `knulli.conf` override of `video_shader_dir` | configgen hard-writes the whole library into `retroarchcustom.cfg` |
| Bezels | `/userdata/decorations` | `/usr/share/knulli/datainit/decorations` is on the squashfs |
| L2+R2 binding | `/userdata/system/configs/multimedia_keys.conf` | `S50triggerhappy` prefers it over `/etc`, which is tmpfs |
| ES ignoring L2/R2 | `/userdata/system/configs/emulationstation/es_input.cfg` | the shipped one is on the squashfs |
| Anything at boot | `/userdata/system/custom.sh` | run by `S99userservices` |

The pattern is that KNULLI already provides a `/userdata` override for nearly
everything, and the work is knowing which one. That knowledge is what the
addon is for — not the patching, which is mostly writing a config line.

## Scope

1. Apply and revert each patch individually, and say which are currently on.
2. Survive an upgrade: after a new image, re-apply what was on before.
3. Sync with a RomM server. This is the part KNULLI cannot do at all, and the
   only part that needs the network.

Reverting matters as much as applying. Several patches this session were wrong
and had to come back off, and doing that by hand meant remembering what the
stock value had been.

## Not decided

Whether the addon owns `knulli.conf` blocks by rewriting between markers — the
`## RomM: ...` / `## RomM: ... end` pairs already in there — or keeps its own
file and merges. Markers are simpler and already work; a separate file is
safer against a user editing inside the block.
