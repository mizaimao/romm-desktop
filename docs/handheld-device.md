# The device, the OS, and what we actually ship

Everything here was read out of
`/Users/frank/Projects/moose-os/romm-sdl2/reference/dArkOS` on 2026-08-21, and
the file it came from is named so it can be re-checked rather than trusted.
Nothing in it is guessed.

`handheld-frontend.md` is the front end. This is the machine it runs on.

## What that repo is

A **build system**, not an image and not a rootfs. A `Makefile`, a
`bootstrap_rootfs-rk3566.sh`, a `create_image.sh`, and a `build_<device>.sh`
per handheld — including `build_miniloong.sh`, the Pocket 1 class we target.
Running it produces a `.img`.

That is a much better position than porting onto a bare device: the device
bring-up is already done and debugged against real hardware.

## Where every piece comes from

| Piece | Source | Buildable from source? |
|---|---|---|
| Base userland | `debootstrap --arch=arm64` from deb.debian.org, cached as a tarball | yes |
| Kernel | `christianhaitian/kernel_5_10_226`, built in-tree | yes, GPL |
| Packages | Debian arm64, apt | yes |
| **U-Boot** | `device/rk3566/uboot.img.anbernic` | **no** — one 4 MB prebuilt blob |
| **GPU userland** | `libmali-bifrost-g52-g29p1.so` (`utils.sh`) | **no** — proprietary ARM blob |
| **Radio firmware** | `firmware/` | **no** |

Three irreducible blobs, then. Everything else is traceable.

**The one that will bite: libmali is paired with the kernel.** `libEGL.so`,
`libGLESv2.so`, `libGLESv3.so`, `libgbm.so` and about twenty other names are
all *symlinks* to `libMali.so` — the whole GL stack is that one file. It has to
match the Mali kbase driver in the kernel it runs against. Mismatch them and
there is no GL at all, with no useful error. **Pin the libmali version
explicitly and change it deliberately.**

## Why we ship an image, and cannot ship a zip

From `setup_partition-rk3566.sh`. GPT, 512-byte sectors:

| Partition | Sectors | What |
|---|---|---|
| `uboot` | 16384–24575 | 4 MB, raw, GUID `A60B0000-0000-4C7E-8000-015E00004DB7` |
| `resource` | 24576–32767 | 4 MB, Rockchip resource |
| `dArkOS_Fat` | 32768–235519 | 104 MB FAT32 — kernel `Image` and the `.dtb` |
| `rootfs` | 237568–15445614 | ~7.7 GB, **btrfs**, `compress=zstd:1` |
| `ROMS` | above that | FAT |

Three things each make "format a card and unzip onto it" impossible:

* **U-Boot is a partition, not a file.** The BootROM looks for it at a fixed
  offset with a specific type GUID. Nothing you copy into a filesystem puts it
  there.
* **The rootfs is btrfs and carries Unix metadata** — ownership, permission
  bits, and the symlink farm that *is* the GL stack. A FAT copy made on macOS
  or Windows cannot express any of it.
* **The partition table is part of the boot contract**, not an artefact of it.

**Why SpruceOS gets away with unzipping:** the Miyoo Mini boots from internal
NAND. Its bootloader and kernel are already on the device and the SD card only
carries userland, on FAT. The device does the hard part. On an ArkOS-lineage
RK3566 handheld the card *is* the whole boot chain, so there is no equivalent.

**So: flash once, then never again.** That is the version of "no flash" that is
actually available, and it is most of the value:

* First install is a `.img.gz` and Balena Etcher or `dd`. Once.
* After that the app updates itself over WiFi, in place. `src/update.rs`
  already does this on the desktop.
* Games live on the separate FAT `ROMS` partition, reachable from any OS, so
  nothing about updating ever threatens the library.

An in-place updater must never touch the partition table. If we ever do need to
reflash, that is a new install, and it should say so.

## Replacing the front end

One systemd unit. `Emulationstation/emulationstation.service` in full:

```
[Unit]
Description=EmulationStation-fcamod
After=firstboot.service

[Service]
Type=simple
User=ark
WorkingDirectory=/home/ark
ExecStart=/usr/bin/emulationstation/emulationstation.sh
Environment="SDL_VIDEO_EGL_DRIVER=libEGL.so"
Restart=on-failure

[Install]
WantedBy=multi-user.target
```

Ours copies that shape exactly, including `SDL_VIDEO_EGL_DRIVER=libEGL.so`.

**Fork the OS repo; do not reinvent it.** Our contribution should be one
`build_romm.sh` and one unit file, so their kernel fixes and new device support
rebase onto us instead of becoming our problem. What we would be re-deriving
otherwise is not the scripts — it is the partition GUIDs the BootROM wants, the
per-device DTBs, the udev rules for `mali0`, `rga` and the backlight, the
`usb_modeswitch` quirks for the Realtek radios, and the libmali/kbase pairing
above. Credit goes in `LICENSES.md`, which the repo already has.

## What is already installed, so nobody bundles it twice

Read out of `needed_packages.txt` and `needed_dev_packages.txt`.

**Fonts are solved.** `fonts-noto-cjk` is in the rootfs, with `libfreetype6`,
`libfontconfig1-dev` and `libfreetype-dev`. So the Japanese and translated
titles have a face to be drawn in without us shipping one, and fallback
resolution is a fontconfig call rather than a hardcoded list of paths. Nothing
to bundle, and no sixteen megabytes of Noto in the binary.

**The whole SDL stack is a dependency already**: `libsdl2-2.0-0` and
`libsdl2-dev`, plus `-image`, `-ttf`, `-mixer`, `-net` and `-gfx`.

* `libsdl2-image` means cover decoding is free — PNG and JPEG, no separate
  image crate to pick and no second decoder to keep fed.
* `libsdl2-dev` being in the **runtime** list means the device can build our
  binary itself. Cross-compilation is a convenience, not a blocker: the
  cheapest first path is `apt install build-essential` on the handheld.

SDL2 itself comes prebuilt for the chipset from
`christianhaitian/rk3566_core_builds` (`build_sdl2.sh`), so we link against a
known-good vendor build rather than one of ours.

## WiFi

**NetworkManager, driven by `nmcli`.** Not raw wpa_supplicant, despite there
being a `build_wpasupplicant.sh` — `dArkOS_Tools/Wifi.sh` uses `nmcli`
throughout, and `bootstrap_rootfs-rk3566.sh` enables NetworkManager at boot.

Their tool is a bash `dialog` TUI on tty1, driven by `gptokeyb` and an
`osk.py` on-screen keyboard. It is not part of EmulationStation, which means
replacing the front end does not remove it — but it looks like a blue terminal
dialog on a 4" screen, so we want our own.

The whole surface is about six commands:

```
nmcli -f IN-USE,SSID,CHAN,SIGNAL,SECURITY dev wifi     # scan
nmcli device wifi connect "SSID" password "PASS"       # join
nmcli -t -f name,device connection show --active       # what we are on
nmcli con up "SSID" / con down "SSID" / con delete     # manage
```

So the networking is nearly nothing. **The work is the on-screen keyboard**,
and it is worth building well because it is the only way to type on the device.

## Do not make anyone type a token

A RomM API token is forty-odd random characters. Entering one through a
thumbstick keyboard, as the very first thing a new device asks for, is the
worst first run we could design.

The card is already mounted on a Mac when it is flashed. **The desktop app
should write `config.toml` onto it** — server URL, token, library paths — so
the handheld boots already configured. Then the only thing ever typed on the
device is a WiFi password: short, memorable, typed once.

That also gives the on-screen keyboard a much easier first job, and it means
the keyboard can land late without blocking anything else.
