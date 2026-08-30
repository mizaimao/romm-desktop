# Hashing the library and checking every dump

A per-file hash store for the Retro SSD, matched against No-Intro, Redump,
TOSEC and MAME, so that every game gets two answers: **is this a good dump**,
and **what should it be called**.

Renaming is not part of this. The database records a proposed name; acting on
it is a separate job, and [devices.md](devices.md) explains why — a filename is
the id for artwork, saves, gamelist entries and RomM rows across four machines.

Agreed with Frank 2026-08-30. Steps below are in order and can be re-run.

## What is covered

31 systems, 9,759 files, about 660 GB.

Excluded, and why:

    switch, ps3, psvita, xbox360    too large to be worth it
    scummvm                         game data folders, no database exists
    dos, easyrpg, macintosh,        empty
    mame, ports, ps4, xbox
    n3ds                            80 .zcci files; no decompressor here
    gc, wii (RVZ), wiiu (WUX)       needs dolphin-tool, which we will not install

`0_BIOS` (170 files) **is** hashed, flagged as BIOS, and dealt with separately
later — a wrong BIOS breaks emulation silently, so it is worth knowing.

## Where it lives

`/Volumes/Retro/_inventory/` — SQLite plus a JSON export. On the SSD rather
than in this repo: it is data, it is large, and another session works in the
repo.

## Schema

One row per file. Two hashes per file, because they answer different questions:
the **container** hash proves a transfer arrived intact, the **inner** hash
identifies the dump. Half of one night's confusion came from conflating them.

| column | |
| --- | --- |
| `device` | `ssd` for now. The same store can later hold server, android, flip |
| `system` | folder name as it is on disk, `gamegear_hidden` and all |
| `path` | relative to the system folder, so nested dirs survive |
| `size` | bytes |
| `container_md5`, `container_sha1` | of the file itself |
| `inner_md5`, `inner_sha1` | of the ROM inside, when it is an archive |
| `inner_name`, `member_count` | what is in the archive |
| `container_format` | `raw`, `zip`, `7z`, `chd`, `iso` |
| `stripped_md5` | header-stripped hash, NES and SNES only — see below |
| `dat_source` | `no-intro`, `redump`, `tosec`, `mame`, several, or none |
| `dat_name` | the canonical filename the match gives |
| `verdict` | `good`, `bad`, `hacked`, `alt`, `unknown`, `error` |
| `proposed_name` | what a rename would use. **Never applied automatically** |
| `status`, `error` | `ok`, `truncated`, `unreadable`, `no-dat-match`, … |
| `group_id` | links multi-disc sets: an `.m3u` and its discs share one |
| `hashed_at` | so it is obvious which rows predate a later change |

When loose files are zipped later, the row keeps its inner hash and gains a
container hash. Nothing has to be recomputed.

## The databases

All four come from one mirror, no account needed:

    https://raw.githubusercontent.com/libretro/libretro-database/master/metadat/<db>/<System>.dat

`no-intro/` for cartridges, `redump/` for discs, `tosec/` for dump flags,
`mame/` and `fbneo-split/` for arcade. A file can match more than one; the row
records which, so "No-Intro says good, TOSEC says `[b]`" is visible rather than
resolved silently in favour of whichever was checked first.

**No-Intro decides for cartridges.** TOSEC's flags are a lead, not a verdict —
two of eighteen files it flagged one night were exact No-Intro matches, and
replacing them would have made the library worse.

## Steps

### 1. Fetch the DATs

One file per system per database, into `_inventory/dats/`. Index them by md5
and by sha1; a lookup is then instant and offline.

### 2. Walk the tree

`find`, not `ls`. Several systems have nested folders — `sfc` has 10, `n64` 3,
`nes` 2, and `N64_All` is 553 zips under region directories. A flat listing
loses them, which has already cost two separate runs.

Normalise names to NFC before comparing anything. macOS and the Flip store the
same CJK characters differently, and a byte comparison reports identical files
as missing.

### 3. Hash

Cartridges first — 6,300 files, ~15 GB, roughly twenty minutes. Then discs.

    raw file        md5 + sha1
    .zip / .7z      container hash, then the inner ROM's hash
                    (record member_count; a 33-member GoodSNES bundle is not a game)
    .nes            also hash with the 16-byte iNES header stripped
    .sfc/.smc       also hash with a 512-byte copier header stripped when size % 1024 == 512
    .iso            hash directly; Redump lists exactly this
    .chd            see below

### 4. CHDs

Read the header first. `chdman info` reports a metadata tag that decides the
method:

- **`Tag='DVD '`, 2048-byte hunks** — the decompressed image *is* the ISO, so
  the Data SHA1 compares straight to Redump. No extraction.
- **`Tag='CHT2'`, 19,584-byte hunks** — CD, multi-track, with subchannel data
  interleaved. Nothing in Redump corresponds to it. `chdman extractcd` to
  cue+bin, hash each track, compare, delete. One disc at a time keeps scratch
  space under a gigabyte.

By system: psp and most ps2 verify for free; **psx (93), dreamcast (25),
3do (10), saturn (4)** need extraction, and those are the small ones.

Also parse the header directly to catch truncation, which `chdman` reports only
as a vague I/O error. If `map offset` is past the end of the file, the file is
short and nothing can be extracted from it. That is how
`Amplitude (USA).chd` (PS2) was found to be missing 46 MB — a 1.4 GB file that
looked fine in a listing and would have got a happy-looking md5 from a plain
hash pass.

`chdman verify` proves a CHD is not internally corrupt. It says nothing about
whether the dump is right. Different question.

### 5. Match and judge

Look up every hash in every DAT. Record the source, the canonical name, and a
verdict. A file that matches nothing is `unknown`, not `bad` — coverage is thin
on Europe-only releases, and RetroAchievements absence proves nothing at all.

Arcade is verified but **never renamed**: for MAME and FBNeo the filename *is*
the set name, and parent/clone loading depends on it.

### 6. Index the references

Before any rename can be trusted, record every place a filename appears:

    ES-DE/gamelists/<system>/gamelist.xml     <path> and <image>
    ES-DE/support/downloaded_media/<system>/  eleven media types per system
    Saves/ and saves/                         .srm, .state
    .m3u contents                             multi-disc playlists
    RomM                                      the roms table on the server

Roughly doubles the build time. It is the difference between a rename that can
be trusted and one that cannot.

### 7. Report, do not act

Per system: how many good, bad, unknown; what would be renamed and to what.
Frank reviews per system before anything moves.

## Decisions already taken

- `Amplitude (USA).iso` (PS2) stays an ISO. Compression to CHD happens later,
  together with zipping the loose cartridge ROMs — one pass, one set of new
  container hashes.
- NES and Famicom stay separate systems. So do SNES and SFC. Merging later is
  easy; splitting is not.
- `_hidden` folder names are kept exactly as they are.
