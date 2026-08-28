# Library audit, 2026-08-28

Every ROM on this machine and on the server, hashed and looked up in No-Intro
and Redump. What follows is what disagrees with those databases, what is
duplicated, and where the two copies of the library have drifted apart.

Short version: **the server is in good shape and the copy on this machine was
not.** 190 files here were named as a game they do not contain, and the server
already held 155 of them under the right name. That has since been fixed by
taking the server's names — "The sync" below — and everything else found is
either recoverable or a decision waiting to be made.

## How it was checked

* 10,103 files under `library/roms`, and 10,069 on `dev.lan` under
  `/home/frank/romm/assets/roms` (top level plus the multi-disc folders; the
  `videos/`, `covers/` and other media folders beside them were skipped).
* Zip and 7z members are identified by the CRC32 already in the archive's own
  directory, so 5,000 archives cost a seek each rather than a decompress.
* NES and SNES files are hashed a second time with the 16-byte iNES header and
  the 512-byte copier header removed, and N64 files are byte-swapped to `.z64`
  order. No-Intro hashes all three that way, and without it `famicom` scores 1
  of 416 rather than 256.
* CHDs are identified by the SHA1 in their own header, and separately verified
  with `chdman verify`.
* Databases: libretro-database's No-Intro and Redump sets, version 2026.08.01 —
  the same vintage as the ones behind `wrong-names.md`.
* RomM itself was read over its API (9,238 entries) and its records compared
  against the files actually on the server's disk.

Nothing was written anywhere but this repo. The server was read only.

## What matches

| system | files | in No-Intro | unknown | name is a different game | title differs |
|---|---|---|---|---|---|
| famicom | 416 | 256 | 160 | 4 | 22 |
| gamegear | 330 | 303 | 27 | 9 | 32 |
| gb | 633 | 553 | 80 | 16 | 70 |
| gba | 799 | 773 | 26 | 1 | 15 |
| gbc | 476 | 468 | 8 | 13 | 36 |
| mastersystem | 352 | 346 | 6 | 6 | 18 |
| megadrive | 942 | 853 | 89 | 108 | 47 |
| n64 | 43 | 40 | 3 | 0 | 0 |
| nds | 3 | 1 | 2 | 0 | 0 |
| neo-geo-pocket | 48 | 24 | 24 | 1 | 7 |
| nes | 852 | 752 | 100 | 10 | 64 |
| pcengine | 290 | 286 | 4 | 8 | 40 |
| sfc | 506 | 498 | 8 | 8 | 38 |
| snes | 493 | 471 | 22 | 5 | 35 |
| wonderswan | 70 | 3 | 67 | 1 | 2 |
| wonderswancolor | 73 | 25 | 48 | 0 | 3 |
| **total** | **6,326** | **5,652** | **674** | **190** | **429** |

Arcade, MAME and Neo Geo AES are not in this table: they are MAME romsets, which
No-Intro does not cover and `data/arcade-core-test.json` already measures a
better way, by launching them.

"Title differs" is the harmless column — `Parasol Stars` against No-Intro's
`Parasol Stars - Rainbow Islands II`, `Tale Spin` against `TaleSpin`. Same
bytes, shorter name. It is listed only so it is not mistaken for the column
beside it.

## The files named as a game they do not contain

190 of them, listed in [lists/audit-name-vs-content.tsv](lists/audit-name-vs-content.tsv).
108 are Mega Drive, which is the set `wrong-names.md` and `md-rename-plan.md`
already describe — and the important new fact is that **that plan has been
carried out on the server and never applied here.** Of the 190, the server holds
155 under the correct name and the other 35 under a different correct name
(often the native title: `信長の野望・覇王伝` where No-Intro says
`Nobunaga no Yabou - Haouden`).

Four of them are not a mislabelled game but a different thing entirely, read out
of the ROM header rather than any database:

| file | what is actually inside |
|---|---|
| `megadrive/Lion King, The (World).zip` | a 16 KB `GENESIS OS` stub |
| `megadrive/Snow Bros. - Nick & Tom (Japan).zip` | CrazyBus, the Venezuelan homebrew |
| `megadrive/Final Zone (Japan, USA).zip` | a Wonder Mega boot ROM |
| `megadrive/Chester Cheetah - Too Cool to Fool (USA).zip` | "Chucks Excellent Art Tool Animator" |

The rest of the non-Mega-Drive entries in that list are mostly a release known
under two names — `Spawn (USA)` for `Todd McFarlane's Spawn`, `Daikatana` for
`John Romero's Daikatana`. Read the list before renaming anything.

## This machine is behind the server

The two libraries were compared file by file, by content rather than by name.

| | count |
|---|---|
| same name here and there, different bytes | 34 |
| same bytes, the server has since renamed the file | 383 |
| same bytes, name differs only in spacing or folder | 1,450 |
| here and nowhere on the server | 58 |

That comparison has since been acted on — see "The sync" below, and
[lists/audit-sync-applied.tsv](lists/audit-sync-applied.tsv) for what moved.

The 34 are the sharp end. 30 are Mega Drive, and in every one of them the
server's copy is the game its name claims and the copy here is not:

    megadrive/NHL 98 (USA).zip          here: NHL 95 (USA, Europe)      server: NHL 98 (USA)
    megadrive/Phantasy Star IV (USA).zip here: Phantasy Star II (Brazil) server: Phantasy Star IV (USA)
    megadrive/Valis (USA).zip            here: Mugen Senshi Valis (Japan) server: Valis (USA)

Three of the remaining four are `mame/` romsets that differ from the server's
`arcade/` copy of the same set, and one is `famicom/B-Wings (Japan).zip`, which
is 112 bytes longer here than the published dump.

Of the 58 files the server does not have, 46 are arcade or Neo Geo romsets
(`quizmoon`, `inufuku`, `lordgun` and friends). The rest:

* Seven the server has **lost** — RomM still lists them and flags them
  `missing_from_fs`: `Barbie-Game Girl.gb`, `Harvest Moon.gb`, `Home Alone 2.gb`,
  `Ecco the Dolphin (USA, Europe).gg`, `Madden NFL '96 (USA, Europe).gg`,
  `Final Zone (Japan, USA).zip`, `Snow Bros. - Nick & Tom (Japan).zip`. This
  machine holds the only copy of the first five; the last two are the broken
  files above, so they are no loss. RomM also lists two more it cannot find,
  `Kat's Run` and `Super Bikkuriman`, which are gone from both sides.
* `psx/Tony Hawks Pro Skater 2 (USA).chd`, 463 MB, which exists nowhere on the
  server. Its name is missing the apostrophe Redump uses.
* Three that are on the server under another folder or another packaging:
  `sfc/Star Ocean (Japan) (Translated En).7z` (server: `snes/`),
  `gba/Mother 3 (English v1.3) (Japan).gba` (server: `Mother 3 (Japan) (Translated En).7z`),
  `nes/Circus Charlie.nes` (server: `CIRCUS.NES`).

RomM's own records were checked against the server's disk as well: every plain
file's md5 agrees with the database row. The apparent disagreements on `.7z` and
`.chd` are an artifact of how RomM hashes an archive — one digest across all
members in sorted order, which `download.rs` documents — not a problem.

## Archives holding more than one dump

419 `sfc` archives and two `megadrive` ones are GoodSNES-era bundles: the same
game several times over, tagged `[!]`, `[b1]` bad dump, `[h1]` header hack,
`[f1]` fixed, plus every translation anyone made. `Bahamut Lagoon` has sixteen
ROMs in it.

That matters because the emulator, not us, decides which member to load, and it
takes the first one:

| what the first ROM in the archive is | archives |
|---|---|
| a header hack `[h]` | 172 |
| a bad dump `[b]` | 138 |
| a fixed or altered dump `[f]` | 69 |
| an overdump `[o]` | 8 |
| the verified good dump `[!]` | 19 |

So `sfc/Emerald Dragon (Japan) (Translated En).7z` starts a known bad dump, and
152 archives whose name promises a translation load the untouched original
instead, because the translated ROM is somewhere further down the archive.
A separate 14 files are labelled as translations and contain nothing but the
plain Japanese ROM.

This is the same on the server — the archives are byte-identical — so it is a
library problem rather than a sync problem. Repacking each of these down to the
one ROM its name claims is the fix, and it is a job for a script.

## Duplicates

97 groups of files with identical content under more than one name, none of
them large:

| | groups |
|---|---|
| wonderswan + wonderswancolor | 19 |
| sfc + snes | 15 |
| gb + gbc | 12 |
| within megadrive | 11 |
| within snes | 11 |
| within sfc | 9 |
| everything else | 20 |

Two structural causes behind most of it. The first is that `wonderswan/` holds
65 `.wsc` files (Colour games) and 5 `.ws`, while `wonderswancolor/` holds 44
`.ws` and 29 `.wsc` — the two folders are filed backwards from each other, and
19 games are in both. The second is the `sfc`/`snes` and `famicom`/`nes` split,
where the same game arrives once under each.

Counting by game rather than by bytes, 41 Game Boy titles, 27 SNES and 24 Mega
Drive are held twice, usually as a No-Intro-named copy beside an untagged one
(`Aladdin (USA).gb` and `Aladdin.gb`).

The server has exactly one duplicate group, and it is a legitimate one: the same
Dizzy compilation released for both Game Gear and Master System.

## Broken, or unusable as stored

* `neogeoaes/matrim.zip` is 0 bytes.
* `arcade/midssio.zip` is 163 bytes.
* `psx/Oddworld - Abes Exoddus (USA).m3u` points at `MultiDisk/...`, and that
  folder does not exist here. The playlist cannot start.
* `psx/Lunar - Silver Star Story Complete (USA)` and `sfc/AdditionalRoms` are
  zips with the extension stripped off — folder ROMs that were downloaded and
  never unpacked. Lunar is three CHDs and its playlist; `AdditionalRoms` is 209
  unlicensed and homebrew SNES ROMs. Neither can be launched as it stands.
* 171 files carry a name No-Intro knows but bytes it does not. 108 of those are
  the exact published length with different content, which is usually a hack or
  a patched ROM wearing the original's name; 35 are longer than the dump
  (overdumps) and 26 shorter.

## Discs

144 CHDs, every one of them internally consistent — `chdman verify` passes on
all of them. 15 are still CHD v4 rather than v5, all Dreamcast; they work, and
converting is optional.

A CHD cannot be matched against Redump by hash the way a cartridge can, since
Redump publishes hashes of the `.bin` tracks and the CHD is a re-encoding of
them. Names were checked instead: of 146 disc images, four are titled something
other than the Redump title, and all four are the same game under a different
release name (`Metropolis Street Racer` against Redump's `MSR - Metropolis
Street Racer`).

## The sets no database recognises

Three folders come from somewhere other than No-Intro and score badly for that
reason rather than because anything is wrong with them:

* `wonderswan`, 3 of 70 matched. The names are a numbered set
  (`021 Shaman King Mirai E no Ishi.wsc`) with Chinese translations among them.
* `neo-geo-pocket`, 24 of 48, GoodTools names (`Pocket Love If (J)`).
* `famicom`'s 160 unmatched, which are almost all Chinese unlicensed carts and
  translations, correctly tagged as such.

Naming conventions across the library: 5,270 files use No-Intro names, 676 use
GoodTools names (`(U) [!]`), and 380 carry no region or dump tag at all. The
untagged ones are concentrated in `gb` (224) and `gbc` (102), and the GoodTools
ones in `nes` (473).

## The sync, carried out 2026-08-28

Steps 1 and part of 5 below are done. The server's names were applied to this
machine:

| | |
| --- | --- |
| renamed in place | 1,701 |
| fetched from the server | 18 |
| parked in `library/replaced-roms/` | 75 |

Nothing was deleted. Every file that moved is listed in
[lists/audit-sync-applied.tsv](lists/audit-sync-applied.tsv), and the parked
files sit under `library/replaced-roms/` with a `sync-manifest.json` beside them
that says where each came from.

Almost all of it was a rename rather than a download, because the bytes here
were already right and only the name had moved on. Renaming happened in two
passes through a temporary name: several games trade names with each other --
what this machine called `NHL 95` is what the server calls `NHL 98`, and the
reverse -- and a single pass would have had the first move overwrite the
second's source.

Parked, rather than deleted, are three kinds of file: a copy of something now
present under the server's name, a loose ROM replaced by the server's zip of the
same game, and four files whose bytes are in no database and on no server
(the Genesis stub, CrazyBus, the Wonder Mega boot ROM and the art tool listed
above).

Afterwards: **6,266 files agree with the server on both name and bytes, and none
disagree.** The count of games the app can find on this machine went from 7,270
to 8,944 of the server's 9,238 — the 1,674 difference is games that were here
all along under a name the app could not match.

52 files were left alone. 32 of those turned out to be copies of games already
here under the server's name and were parked; the remaining 20 are the ones the
server does not have at all, listed above.

## The deletions, propagated

The rename pass moved nothing out of the way, so everything deleted on the
server was still sitting here. A second pass mirrored the server properly:
**every file RomM does not list has been moved to `library/replaced-roms/`.**
976 files, 4.8 GB.

| | files | |
| --- | --- | --- |
| `mame/` | 750 | the server has no `mame` platform any more; `tools/romm_sync.py` still carries code for one, so it did once. 638 of these duplicate `arcade/` romsets already here |
| `neogeoaes/` extras | 131 | arcade romsets filed under Neo Geo AES, which the server keeps in `arcade/` |
| `arcade/` strays | 86 | of which 53 are the Japanese quiz purge recorded in `data/removed-japanese-quiz.json` — all 49 romsets were still here |
| cartridges and discs | 9 | mostly the same game in a different wrapper than the server uses |

Before parking anything, each file was checked for whether the game survives:
every one of those 976 is either a copy of something still present under the
server's name, or a romset the server no longer has. The single game that left
the library outright is `psx/Tony Hawks Pro Skater 2 (USA).chd`, which the
server does not have under any name.

The two folder ROMs that arrived as extension-less zips were unpacked into
directories the way the server stores them: `psx/Lunar - Silver Star Story
Complete (USA)` (three discs and a playlist) and `sfc/AdditionalRoms` (209
unlicensed and homebrew SNES ROMs).

**Nothing under `library/roms` is now unknown to RomM**, and 8,944 of the
server's 9,238 games are here. The 294 that are not have simply never been
downloaded — 108 PSP, 69 DS, 55 GameCube.

## What is left to do

Steps 1, 5 and 6 are done, along with the duplicate half of 4. The rest stands.

1. ~~Re-download the wrongly-named files.~~ Done by rename, above.
2. **Push the five games only this machine still has** back to the server, and
   let RomM rescan so its `missing_from_fs` flags clear. `Tony Hawks Pro Skater
   2` too, renamed to Redump's spelling.
3. **Repack the 419 multi-dump `sfc` archives** down to one ROM each. Until then
   138 of them start a bad dump and 152 that promise English start in Japanese.
   Both copies have the same archives, so this is a job to do once, on the
   server, and let the renamed files come back down.
4. **Pick a side for `wonderswan` versus `wonderswancolor`.** The server files
   them the same way this machine did, so the mixing is on both sides: `.wsc`
   games sitting in the mono folder and `.ws` games in the colour one. The 18
   local copies that duplicated a game already present the other way round have
   been parked; the folders themselves are still crossed.
5. ~~The 0-byte `neogeoaes/matrim.zip` and 163-byte `arcade/midssio.zip`.~~
   RomM lists neither, so both were parked with the rest.
6. ~~Unpack the two folder ROMs that arrived as extension-less zips.~~ Done.
   The Oddworld playlist still points at a `MultiDisk/` folder that is not here;
   RomM lists the playlist but not the discs beside it, which is worth a look on
   the server.
7. The five games RomM has lost are still here and still listed by RomM, so the
   mirror kept them: `Barbie-Game Girl.gb`, `Harvest Moon.gb`, `Home Alone 2.gb`,
   `Ecco the Dolphin.gg`, `Madden NFL '96.gg`. Upload them and this machine stops
   being the only copy.

`wrong-names.md` and `md-rename-plan.md` describe step 1 from the server's side
and are now finished business on both: the renames they ask for were done there,
and this machine has taken them.
