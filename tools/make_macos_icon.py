#!/usr/bin/env python3
"""Rebuild icon.icns as full-bleed art, for macOS 26 (Tahoe) and later.

Tahoe applies its own rounded-square mask to every app icon. An icon that ships
its own rounded plate inside a transparent margin therefore gets *placed inside*
the system container rather than becoming it -- which reads as a small card
floating on a larger tile. The fix is to hand macOS square, edge-to-edge art and
let it do the masking.

Only the .icns is changed. Windows (.ico) and Linux (.png) do no masking, so
those keep the rounded plate they need.

    python3 tools/make_macos_icon.py            # rebuild
    python3 tools/make_macos_icon.py --check    # report, change nothing
"""

import subprocess
import sys
from pathlib import Path

from PIL import Image

ICONS = Path(__file__).resolve().parent.parent / "src-tauri" / "icons"
SIZES = [16, 32, 64, 128, 256, 512, 1024]


def plate_bounds(im):
    """Bounding box of the opaque plate, ignoring the transparent margin."""
    alpha = im.getchannel("A")
    box = alpha.point(lambda a: 255 if a > 128 else 0).getbbox()
    if box is None:
        raise SystemExit("icon.png is fully transparent")
    return box


def full_bleed(src):
    """Square, edge-to-edge version of `src` with its corners filled in.

    The plate is cropped out of its margin and scaled to the full canvas. Its
    own corners are still rounded, so the background behind them is rebuilt by
    sampling one background column per row -- the art has a vertical gradient,
    and a flat fill would show as a band across the corners.
    """
    im = Image.open(src).convert("RGBA")
    plate = im.crop(plate_bounds(im))
    n = max(plate.size)
    plate = plate.resize((n, n), Image.LANCZOS)

    # x=2% is inside the plate but outside the artwork on every row.
    px = plate.load()
    background = Image.new("RGBA", (n, n))
    bg = background.load()
    sample_x = max(1, n // 50)
    for y in range(n):
        r, g, b, a = px[sample_x, y]
        if a < 200:  # a rounded corner: reuse the nearest solid row
            r, g, b, a = px[sample_x, min(n - 1, max(0, n // 2))]
        for x in range(n):
            bg[x, y] = (r, g, b, 255)

    background.alpha_composite(plate)
    return background


def main():
    check = "--check" in sys.argv
    src = ICONS / "icon.png"
    art = full_bleed(src)

    corners = [art.getpixel(p) for p in [(0, 0), (art.width - 1, art.height - 1)]]
    print(f"source {src.name}: {Image.open(src).size[0]}px, plate {plate_bounds(Image.open(src).convert('RGBA'))}")
    print(f"full-bleed corners now opaque: {all(c[3] == 255 for c in corners)}")
    if check:
        return

    iconset = ICONS / "macos.iconset"
    iconset.mkdir(exist_ok=True)
    for size in SIZES:
        img = art.resize((size, size), Image.LANCZOS)
        if size <= 512:
            img.save(iconset / f"icon_{size}x{size}.png")
        if size >= 32:
            img.save(iconset / f"icon_{size // 2}x{size // 2}@2x.png")

    out = ICONS / "icon.icns"
    subprocess.run(["iconutil", "-c", "icns", str(iconset), "-o", str(out)], check=True)
    print(f"wrote {out} ({out.stat().st_size // 1024} KB)")


if __name__ == "__main__":
    main()
