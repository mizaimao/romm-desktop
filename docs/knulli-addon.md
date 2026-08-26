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
| ES's loading logo | blank PNG over `resources/logo.png`, from `/boot/boot-custom.sh` | `/usr` is tmpfs; and it must land before `S31` starts ES |
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

## Plan

### Shape

A new crate, `src-addon`, in the workspace beside the other two. It depends on:

- `romm-sdl` **as a library** — `gfx`, `text`, `input`, `glass`, `backdrop`,
  `status`, `keyboard` are a working SDL/GLES interface already tuned for this
  exact screen: 640×480, the 1.5 scale, the pad read out of `es_input.cfg`,
  text through cosmic-text with font fallback. That work is done and it would
  be silly to redo it. The RomM-specific modules — `library`, `covers`,
  `rescan`, `iconfetch` — stay behind.
- `romm-desktop` — only for the RomM client, and only for the sync part.

### One idea: a patch

    trait Patch {
        fn id(&self) -> &str;
        fn title(&self) -> &str;
        fn why(&self) -> &str;              // one line, shown in the list
        fn state(&self) -> Result<State>;   // Off | On | Changed
        fn apply(&self) -> Result<()>;
        fn revert(&self) -> Result<()>;
    }

`Changed` is the one that earns its keep: KNULLI shipped a new image and the
file underneath is not what we wrote. The addon should say so rather than
silently overwrite or silently skip.

Revert is not a nicety. Several patches this session turned out to be wrong,
and taking them back off by hand meant remembering the stock value.

### Two kinds of patch, and only two

**A marked block in a config file.** `knulli.conf` takes almost everything —
hotkeys, shader set, shader cycling directory, bezel choice. Apply rewrites
between markers, revert deletes between them, state compares:

    ## romm-addon: hotkeys
    ...
    ## romm-addon: hotkeys end

One mechanism, and it is already proven — the blocks in `knulli.conf` right
now have exactly this shape.

**A file we place.** Bezel PNGs, shader presets, `es_input.cfg`,
`multimedia_keys.conf`, `custom.sh`. These need a backup of whatever was there
first, so revert can put it back. `es_input.cfg` and `multimedia_keys.conf` are
both "copy the shipped one, change two things" — that pattern is worth having
once rather than three times.

### The profile

`/userdata/system/romm-addon/profile.toml`: which patches are on, and their
options. This is the answer to *"even on a newly installed KNULLI we can easily
configure and recover all of those customized settings"* — a fresh device means
copy the binary and the profile, run apply-all.

Which means **the binary has to carry its own assets**. A bezel is a PNG and a
shader preset is a text file; if they live beside the binary then recovery is a
directory to remember rather than a file. Embedded, recovery is one file plus
one profile.

### Where it got to

Steps 1 to 3 are done and running on the device.

`patch.rs` is the engine: a patch is a list of steps, a step is either a marked
block in a text config or a file we own, and both can be asked whether they are
already satisfied. That last part is what lets the menu open at what the device
*actually is* rather than at what it ought to be — and lets it say **changed**
when a patch sits at none of its options, which is what a KNULLI update looks
like from in here.

`catalogue.rs` is ten patches, with their bodies and their files compiled in.
`profile.rs` is the recovery story: `--save`, `--restore`, `--status`, all of
which run without a window, because a device that has just been reflashed
cannot launch a windowed app until some of these patches are on.

Two things the device taught the engine, neither of which was guessed:

`/boot` is mounted **read-only**, and `boot-custom.sh` has to live there because
it is the only hook that runs before EmulationStation starts. So a write that
comes back `EROFS` gets one retry with the mount flipped, and the mount is put
back afterwards whichever way it goes.

A patch that fails must not abandon the ones after it. The first restore on the
real device hit that read-only mount and lost every setting that came later in
the file — on a freshly flashed handheld that is the difference between one
thing to fix and ten.

### Still to do

The three sync actions are drawn but not wired. `romm_desktop::savesync` already
has `SaveConflict`, `Keep` and `Summary`, so this is plumbing rather than design.

`fast-launch` — the preforked configgen daemon, measured at 1241 ms down to
7.9 ms — is **not** in the catalogue. It is the only one of these that replaces
the program that starts games, and shipping it untested alongside nine patches
that only write config would be trading a real risk for a second saved.

### Order

1. The crate, the `Patch` trait, the marked-block mechanism, and two real
   patches — hotkeys and shaders — with apply, revert and state.
2. The file-placement mechanism, with backups, and the rest of the patches.
3. Profile: save, load, apply-all. This is the part that has to work on a
   device that has never seen the addon.
4. The interface. Three screens: patches, sync, status.
5. Sync.

Steps 1–3 are usable from a shell before there is any interface at all, which
is the order that gets it tested.

### Undecided

The name. `romm-addon` is wrong — it patches KNULLI and only one of its jobs is
RomM.

Which half of sync comes first: pushing saves up, pulling saves down, or taking
games offline.
