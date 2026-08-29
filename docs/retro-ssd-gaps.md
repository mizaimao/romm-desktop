# What is still missing from the Retro SSD

The Retro SSD was brought in line with the server on 2026-08-29: 1,558 ROMs
renamed to the server's names, 15,484 media files carried with them, 1,470
gamelist entries patched, 157 files copied in, 137 quarantined. Name
mismatches went from 1,717 to 11.

These 24 did not land. Not built into any sync, just written down.

## The list

| server slug | Retro folder | file |
| --- | --- | --- |
| 3do | 3do | Star Control II (USA, Europe).chd |
| arcade | arcade | nslasherj.zip |
| dc | dreamcast | Ikaruga (Japan).chd |
| gamegear | gamegear_hidden | Power Strike II (Japan, Europe) (En).zip |
| gamegear | gamegear_hidden | Sonic Drift 2 (World).zip |
| gb | gb | Wario Land - Super Mario Land 3 (World).zip |
| gba | gba | Mario Tennis - Power Tour (USA, Australia) (En,Fr,De,Es,It).zip |
| gbc | gbc | Tetris DX (World) (SGB Enhanced) (GB Compatible).zip |
| mastersystem | mastersystem_hidden | Sagaia (Europe, Brazil) (En).zip |
| mastersystem | mastersystem_hidden | Sonic Chaos (Europe, Brazil) (En).zip |
| megadrive | genesis | Lightening Force - Quest for the Darkstar (USA).zip |
| megadrive | genesis | NHLPA Hockey 93 (USA, Europe, Rev 1).zip |
| nds | nds | Phoenix Wright - Ace Attorney (USA).7z |
| neogeoaes | neogeo | hng64.zip |
| ngc | gc | Tales of Symphonia (USA) |
| pcengine | pcengine_hidden | Bomberman '94 (Japan).zip |
| pcengine | pcengine_hidden | Lords of Thunder (USA).chd |
| psx | psx | Spyro the Dragon (USA).chd |
| saturn | *(no folder)* | Keio Flying Squadron 2 (Europe).chd |
| saturn | *(no folder)* | Nights Into Dreams... (USA).chd |
| saturn | *(no folder)* | Radiant Silvergun (Japan).chd |
| saturn | *(no folder)* | Saturn Bomberman (USA).chd |
| sfc | sfc | Kat's Run - Zen-Nihon K-Car Senshuken (Japan).zip |
| snes | snes | Breath of Fire II (Europe).zip |

## Three different problems, not one

**Most are not actually missing.** Roughly half are the same game already on
Retro in a different container — the server keeps a `.zip`, Retro keeps the raw
ROM. `Power Strike II` and `Sonic Drift 2` are on Retro as `.gg`,
`Bomberman '94` as `.pce`, `Sonic Chaos` and `Sagaia` as `.sms`. The sync
deliberately keeps Retro's format rather than renaming a raw ROM to `.zip`,
which would produce a file no emulator can open. Nothing to fetch; they are
only "missing" to a comparison that matches on full filename.

**`saturn` has no folder on Retro at all.** Four CHDs, and a directory would
have to be created first. Decide whether Saturn belongs on this device before
copying them.

**A handful are genuine.** `Star Control II`, `Ikaruga`, `Lords of Thunder`,
`Spyro the Dragon`, `Tales of Symphonia` (a folder), `nslasherj`, `hng64`, and
the `.7z` Phoenix Wright. These are real absences and a plain copy fixes them.

## Picking it up

The comparison is hash-based, not name-based: RomM hashes the ROM *inside* an
archive, and for a multi-member arcade zip it hashes every member's bytes
concatenated in zip order. `unzip -p file | md5` reproduces it exactly. See
[fast-launch.md](fast-launch.md) for the same lesson in a different context —
verify against the thing itself, not against what the name says.
