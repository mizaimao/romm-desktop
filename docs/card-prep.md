# Preparing a card — standard procedure

Four firmwares done this way so far: NextUI (RG SP), spruceOS (Miyoo A30, Miyoo
Flip), dArkOS (MiniLoong Pocket 1), KNULLI (RK3566). The steps are the same
every time; only the layout table changes.

Read `docs/a30-spruce-card.md` for the long-form story of the first one. This is
the short version you actually follow.

## The sequence

1. **Identify the card before touching it.** `diskutil list`. A card in a Mac's
   built-in reader reports as **internal**, so `diskutil list external` shows
   nothing and the habit of "pick the external one" does not protect you. Two
   128 GB cards look identical by size — tell them apart by volume name.
2. **Install the firmware.** Image flash (`dd`) or extract-to-card, depending.
   See the table.
3. **Boot it once on the device.** Non-negotiable. First boot creates the
   folder tree, expands partitions, and in spruce's case unpacks ~28 nested
   archives. Never guess folder names — read them off the card afterwards.
4. **Survey the card, do not assume.** Which system folders exist, what art
   convention, where saves and BIOS go. Every firmware has surprised us at
   least once.
5. **Build the manifest and show it** before copying. Platforms the device
   cannot run are a waste of hours; platforms with no folder are impossible.
6. **Stage**: ROMs, art, gamelists (if ES-based), BIOS, saves, collections,
   favourites, cores.
7. **Verify, then eject.** Checklist at the bottom.

## Layout, by firmware

| | NextUI | spruceOS | ArkOS / dArkOS | KNULLI / Batocera |
|---|---|---|---|---|
| ROM folders | `Roms/<Name> (TAG)` | `Roms/<SYS>` | `<system>` on EASYROMS | `roms/<system>` on SHARE |
| Folder naming | tag in parens maps the emulator | fixed short names | lowercase ES names | lowercase ES names |
| Art path | `.media/<stem>.png` | `Imgs/<stem>.png` | `images/<stem>.png` | `images/<stem>-image.png` |
| Art size | screen-sized | **640x480**, converted to QOI on device | free choice | free choice |
| Gamelist | none | none | `gamelist.xml` | `gamelist.xml` |
| Favourites | n/a | `Saves/pyui-favorites.json` | `<favorite>true</favorite>` | `<favorite>true</favorite>` |
| Collections | n/a | `Collections/collections.json` | ES `custom-*.cfg` (on rootfs) | `system/configs/emulationstation/collections` (on SHARE) |
| Saves sorted by | core | **core**, with arch suffix | core | **system** |
| Per-system core | folder tag | `Emu/<SYS>/config.json` | `<alternativeEmulator><label>` | `<system>.core=` in `knulli.conf` |
| Per-game core | none | none | `<altemulator>LABEL</altemulator>` | `<emulator>`/`<core>` in gamelist |
| BIOS | `Bios/<TAG>/` | `BIOS/` flat + `BIOS/fbneo/` | `bios/` flat on EASYROMS | `bios/` flat on SHARE |

## The traps, in the order they bit us

**Format under Windows for spruce.** A macOS-formatted card extracted perfectly
— 10,634 of 10,634 files, `.tmp_update` present, every dot path intact — and hung
on the Loading screen past 40 minutes. Two wasted install cycles. Rufus, FAT32.
Image-flashed firmwares (dArkOS, BaseOS) do not care, because `dd` writes the
partition table itself.

**AppleDouble files.** Finder writes one `._name` sidecar per file onto
FAT32/exFAT and MinUI-family firmware lists them as games. `dot_clean -m` over
every folder you touched, including `images/`, which is created after the copy
loop and gets missed. Delete `.fseventsd`; `.Spotlight-V100` cannot be removed
and does not matter.

**The architecture suffix.** spruce names save folders from
`spruce/scripts/emu/lib/core_mappings.sh`, not from the core's display name, and
appends the arch: `PCSX-ReARMed-32` on the A30 (armhf), `PCSX-ReARMed-64` on the
Flip (aarch64). Reading the display name out of the `.so` gives the wrong answer.
Check `PLATFORM_ARCHITECTURE` in the platform cfg first.

**Save extensions differ by core, and a folder-level copy is not enough.**
Measured from a real backup:

| Core | Writes |
|---|---|
| Geolith | `.mcr` + `.nv` |
| FB Alpha 2012 | `.fs` |
| FinalBurn Neo | `.fs` + `.hi` |
| Flycast | `.ldci` |
| mGBA, Snes9x, Gambatte, PCSX-ReARMed, SwanStation, Mupen64Plus | `.srm` |

Copying Geolith's `.mcr` into an FB Alpha 2012 folder produces files that core
can never read. **Match the core, not just the system.** Battery saves are raw
dumps and do port between cores of the same system; save *states* never do.

**Device-specific core defaults.** spruce's SFC default is `chimerasnes` on the
A30 but `mednafen_supafaust` on the Flip — the config carries per-device blocks
keyed on `MIYOO_FLIP`. Read the block that names your device, not
`default_emulator`.

**Cores missing from the map.** geolith is absent from `core_mappings.sh`, so
`get_core_folder` returns `"UNKNOWN"` and every save lands in a shared junk
folder. Adding one case line fixes it.

**Junk rows in the library.** Four server entries are directories indexed as
single games and fail every copy: `Battle City`, `Multicarts`, and two
`Aftermarket`. N64 additionally has `USA` and `Europe` — whole region folders,
6.5 GB between them. Filter those or N64 reports 45 games at 7.2 GB.

**Multi-disc games.** Folders of CHDs with an `.m3u`. `stage_arkos_card.py` and
`stage_knulli_card.py` handle them; `stage_spruce_card.py` does not and needs a
manual pass. Layout is a dot-prefixed folder for the discs with the playlist
beside it, so only the playlist shows in the list.

**Neo Geo naming.** The server stores titles (`2020 Super Baseball (set 1).zip`);
FBNeo and FB Alpha identify sets by MAME romset id (`2020bb.zip`). Only 8 of 163
match, so those cores fail on the rest. Either use geolith, or rename via
`data/arcade-names.json`, which maps 147 of 163.

**Silent path validation.** PyUI drops any collection or favourite whose
`rom_file_path` does not resolve, without a word. Build those against what is
actually on the card and verify zero broken paths before ejecting.

## Tools

    tools/build_media_set.py     resize art once into library/media-640, reused per card
    tools/stage_spruce_card.py   spruceOS
    tools/stage_arkos_card.py    ArkOS / dArkOS
    tools/stage_knulli_card.py   KNULLI / Batocera
    tools/stage_sp_card.py       NextUI
    tools/copy_to_card.py        top up an existing ES-DE card from the server
    tools/card_collections.py    ES-DE collections and favourites

All are dry by default; `--apply` is the only thing that writes.

## Before ejecting

```sh
find /Volumes/<CARD> -name '._*' | wc -l          # must be 0
lsof +D /Volumes/<CARD>                            # must be empty
python3 -c "import json;json.load(open('<collections.json>'))"   # PyUI cards
```

For ES-based cards, confirm every gamelist parses — they carry two root
elements (`<alternativeEmulator>` then `<gameList>`), which is not well-formed
XML, so wrap before parsing.

Then `diskutil eject`, or `sudo umount` if it was mounted by hand.

## Recovery point

`dd` cannot skip free space device-to-device. Compress instead — free space
costs almost nothing:

```sh
sudo diskutil unmountDisk /dev/diskN
sudo dd if=/dev/rdiskN bs=4m | gzip -1 > card-$(date +%Y%m%d).img.gz
```

Restore with `gunzip -c ... | sudo dd of=/dev/rdiskN bs=4m`. No need to wipe the
target first. Verify the archive once with `gunzip -t` — a backup that has never
been read is not one.
