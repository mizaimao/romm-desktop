# The library rules

**Every file hash-verified. Every filename the consensus database-registered
name.** Both halves, always — renaming is part of the job, not a separate
question to ask.

Frank had to repeat this more than ten times before it stuck. It is not a
preference to confirm per batch; it is the definition of what the library is.

The rest of this file is how that rule is applied, and the mechanics that have
gone wrong at least once. [inventory.md](inventory.md) is the procedure and the
schema; this is the policy.

## Naming

A file's name comes from **its own hash**, never from the game it resembles.

- An alternate keeps its alternate name. A bad dump keeps a name that says
  `[b]`. A translation keeps its translator tag.
- Variant markers — `[a1]`, `[a2]`, `(Alt)`, `[b2]`, `[p1]`, `(T)` — are
  identity, and are never stripped when matching. Stripping them once matched
  seven NES alternates to the consensus dump and deleted them.
- CJK filenames stay CJK. Everything else takes the database name.
- **Arcade, arcade_original and neogeo are never renamed.** The filename is the
  MAME/FBNeo set name the emulator looks up, and parent/clone loading depends
  on it.

`(T)` in a filename means Translation. Patched ROMs are catalogued nowhere,
which is exactly how you recognise one: the plain release matches a database,
the `(T)` copy matches nothing.

## Databases, in order

No-Intro for cartridges, Redump for discs, TOSEC via Hasheous when neither
knows the hash. Record which matched. **No-Intro decides over TOSEC** when they
disagree — two of eighteen files TOSEC flagged one night were exact No-Intro
matches, and acting on the flag would have made the library worse.

Per-console quirks that decide whether a hash matches at all:

    NES / Famicom   iNES header is 16 bytes. Hash the body.
    SNES / SFC      No-Intro is HEADERLESS. Strip a 512-byte copier header
                    when size % 1024 == 512. The opposite of NES.
    N64             three byte orders; No-Intro lists big-endian (z64)
    Arcade          RA hashes the *filename*, not the contents
    CD-based CHD    needs extraction; DVD-based compares straight off the header

## Try the transform before sourcing anything

Trimmed ROMs pad to full cart size — `0xFF` usually, sometimes `0x00`.
Overdumps truncate. Headered versus headerless is a hashing difference, not a
different file.

Eleven Mega Drive files and one NES were fixed this way with no download at
all. Always try it before asking Frank to find something.

## What gets removed

- **Bad dumps**, when a good one exists. If the only dump in circulation is
  bad, keep it and let the name say so.
- **Prototypes and betas**, only when the game had a real release. For a
  proto-only game the prototype *is* the game — check the DAT for a non-proto
  entry rather than guessing.
- **Duplicates**, where the same ROM sits under two names.

Everything removed moves to the session scratch folder. Nothing is deleted
outright.

## What is kept but never verified

Fan translations and hacks. No database has ever registered a hash for them, so
they can be neither verified nor renamed. That is a floor, not a backlog — do
not report it as outstanding work.

## Imports

One region per title, preferring **USA > World > Europe > Japan**. Skip anything
already held, by hash *and* by title. Exclude betas, protos, demos and
plug-and-play compilation variants. Name from the database at the moment the
file lands, never carry in the source set's filename.

Unlicensed titles go to their own folder — `nes_unlicensed`, sorted into
`Bootleg Ports`, `Originals` and `Multicarts`.

## Mechanics that have gone wrong, and are now guarded

- **exFAT is case-insensitive.** `BattleTech` and `Battletech` are the same
  file. A case-only rename goes via a temporary name; copy-then-delete destroys
  it. Three Mega Drive files were lost this way and restored from the payload.
- **Never overwrite an existing file at the target name.** Check first whether
  it is the same ROM: 54 blocked renames turned out to be duplicates the import
  had created, 52 the same ROM under a different iNES header.
- **A rename carries everything named after the file** — artwork across eleven
  media types, saves, `.m3u` entries, the gamelist entry, the RomM row.
- **Normalise to NFC before comparing names.** macOS and the Flip store the
  same CJK characters differently; a byte comparison calls identical files
  missing.
- **A folder name is not evidence.** 80 of 124 WonderSwan files were in the
  wrong system and 7 Neo Geo Pocket files had the wrong extension. The hash
  decides which system a file belongs to.
- **A set is dated.** A file that fails a current DAT while matching an older
  one is stale, not broken.

## Scope

SSD only. Server, Android and Flip get pushed once the canonical copy is right.

## Where it stands

See [inventory.md](inventory.md). Finished: NES, Famicom, Mega Drive,
WonderSwan, WonderSwan Color, Neo Geo Pocket. Next: `sfc`.
