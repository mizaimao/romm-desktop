# Licences of things this app ships

The app itself is in this repository. These are the parts that are not, and
what their licences ask of us.

## Fonts — SIL Open Font License 1.1

Noto Sans, and the Arabic, Hebrew and Thai families beside it, from the
[Noto project](https://github.com/notofonts/notofonts.github.io).

Fetched at build time rather than committed: pinned by URL and verified by
SHA-256 in `assets/fonts/MANIFEST.tsv`, downloaded by
`scripts/fetch-fonts.sh`. A repository is a poor place to keep two megabytes
of somebody else's binary, and a hash in a text file gets everyone the same
bytes just as well.

**What the licence asks.** OFL 1.1 permits redistribution, bundling and sale
as part of a larger work. Three conditions matter to us:

* the licence travels with the fonts — `OFL.txt` is in the manifest and is
  fetched alongside them, and lands wherever they do;
* they are not sold on their own, which we do not do;
* they keep their reserved names, so we must not modify a font and still call
  it Noto. We do not modify them at all.

**CJK is not fetched by default, deliberately.** Both targets already have it:
the handheld's rootfs installs `fonts-noto-cjk`, and macOS ships PingFang and
Hiragino. It is about 50 MB against the 2.3 MB of everything above. A machine
that needs it — a Linux desktop without the Debian package, or Windows — gets
it with `./scripts/fetch-fonts.sh --with-cjk`, and those four are OFL 1.1 as
well. Where a machine lacks a CJK face entirely the app says so at startup
rather than drawing empty boxes.

## Icons — ISC

Lucide, vendored into `ui/icons/`. See `ui/icons/README.md`.

## The handheld image

Built from [dArkOS](https://github.com/christianhaitian/arkos), which is a
build system rather than an image and carries its own `LICENSES.md`. What it
assembles is Debian arm64, a GPL kernel, and three binaries that are neither:
the vendor U-Boot, ARM's Mali userspace driver, and radio firmware. See
`docs/handheld-device.md` for where each comes from.
