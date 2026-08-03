#!/usr/bin/env python3
"""Turn the chosen artwork into a macOS app icon and the rest of Tauri's set.

macOS does not mask app icons — whatever shape you supply is the shape shown.
So the rounded tile has to be baked in, at Apple's proportions rather than an
approximation:

* 1024x1024 canvas
* the tile is 824x824 centred, i.e. 80.5% of the canvas, leaving the margin
  Apple's grid expects so icons line up optically in the Dock
* corners are a **superellipse**, not a circular arc. A circular-arc "rounded
  rectangle" sits visibly wrong next to real macOS icons; the continuous curve
  is the thing that makes it look native.
* a soft contact shadow under the tile, which the template bakes in too

Small sizes get their own treatment. A 16px icon is 16 pixels; the full shelf
turns to mush there, so those sizes crop in on the artwork. Supplying different
art per size is exactly what an .icns is for.
"""

import argparse
import math
import pathlib
import shutil
import subprocess
import sys

from PIL import Image, ImageDraw, ImageFilter

CANVAS = 1024
TILE = 824               # Apple's macOS app-icon grid
CORNER_N = 5.0           # superellipse exponent; ~5 matches Apple's curve
RADIUS = 185.4           # Apple's corner radius for an 824px tile

# Tile gradient. Warm near-white to a deeper sand, echoing the artwork's own
# ground so the colourful spines stay the loudest thing in the icon.
GRAD_TOP = (252, 250, 246)
GRAD_BOTTOM = (222, 213, 198)
SS = 4                   # supersampling for a clean edge

# Every size an .icns wants, as (pixel size, [iconset names]).
ICONSET = [
    (16, ["icon_16x16.png"]),
    (32, ["icon_16x16@2x.png", "icon_32x32.png"]),
    (64, ["icon_32x32@2x.png"]),
    (128, ["icon_128x128.png"]),
    (256, ["icon_128x128@2x.png", "icon_256x256.png"]),
    (512, ["icon_256x256@2x.png", "icon_512x512.png"]),
    (1024, ["icon_512x512@2x.png"]),
]


def squircle_mask(size, side, radius=RADIUS, n=CORNER_N):
    """Rounded square with Apple's continuous corners.

    The curve applies **only at the corners**, joined by straight edges; running
    a superellipse across the whole square bows the sides and the icon reads as
    a blob. The four arcs must also be emitted in perimeter order — clockwise
    from the top edge — or the polygon self-intersects into a bowtie.
    """
    big = size * SS
    m = Image.new("L", (big, big), 0)
    d = ImageDraw.Draw(m)
    s, r = side * SS, radius * SS
    off = (big - s) / 2.0
    x0, y0, x1, y1 = off, off, off + s, off + s

    steps = 192
    pts = []

    def sweep(cx, cy, fx, fy):
        """One corner: `fx`/`fy` map (u, v) onto the right quadrant."""
        for i in range(steps + 1):
            a = (math.pi / 2) * i / steps
            u = abs(math.cos(a)) ** (2.0 / n)
            v = abs(math.sin(a)) ** (2.0 / n)
            pts.append((cx + fx(u, v) * r, cy + fy(u, v) * r))

    # Clockwise: top edge -> TR -> right edge -> BR -> bottom -> BL -> left -> TL
    sweep(x1 - r, y0 + r, lambda u, v: v,  lambda u, v: -u)   # top-right
    sweep(x1 - r, y1 - r, lambda u, v: u,  lambda u, v: v)    # bottom-right
    sweep(x0 + r, y1 - r, lambda u, v: -v, lambda u, v: u)    # bottom-left
    sweep(x0 + r, y0 + r, lambda u, v: -u, lambda u, v: -v)   # top-left
    d.polygon(pts, fill=255)
    return m.resize((size, size), Image.LANCZOS)


SENTINEL = (255, 0, 255)


def cut_subject(im):
    """The subject with its background removed, as RGBA.

    Built from what the subject *is* rather than by flood-filling the
    background. The render's ground carries both a dark cast shadow and a
    bright highlight enclosed by it; a flood fill from the borders cannot reach
    that highlight, and no threshold removes the shadow without also eating the
    books.

    So: mark every pixel that is saturated (the coloured covers) or genuinely
    dark (their outlines), then treat anything *not* marked and reachable from
    the border as background. What survives is the covers plus the near-white
    page blocks they enclose — holes, not background.
    """
    src = im.convert("RGB")
    w, h = src.size
    sp = src.load()

    solid = Image.new("L", (w, h), 0)
    sop = solid.load()
    for y in range(h):
        for x in range(w):
            r, g, b = sp[x, y]
            hi, lo = max(r, g, b), min(r, g, b)
            if (hi - lo) > 34 or hi < 105:
                sop[x, y] = 255

    # Flood the un-marked area inward from each corner; whatever it reaches is
    # true background. Enclosed gaps (page blocks) are never reached.
    marked = solid.copy()
    for corner in ((0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)):
        if marked.getpixel(corner) == 0:
            ImageDraw.floodfill(marked, corner, 128, thresh=0)

    alpha = marked.point(lambda v: 0 if v == 128 else 255)
    alpha = alpha.filter(ImageFilter.GaussianBlur(0.8))

    out = src.copy()
    out.putalpha(alpha)
    return out, alpha.getbbox()


def gradient(top, bottom):
    """Vertical two-stop gradient across the full canvas."""
    g = Image.new("RGB", (1, CANVAS))
    for y in range(CANVAS):
        f = y / (CANVAS - 1)
        g.putpixel((0, y), tuple(int(top[i] + (bottom[i] - top[i]) * f) for i in range(3)))
    return g.resize((CANVAS, CANVAS), Image.BICUBIC)


def build(subject, box, px, zoom, grad):
    """One icon at `px`: gradient tile, subject on top. Exactly two elements.

    The subject is cut out rather than pasted as a rectangle of its own
    background — that rectangle was visible inside the tile and read as a
    second, inner layer.
    """
    tile_mask = squircle_mask(CANVAS, TILE)
    tile = grad.copy()

    bx0, by0, bx1, by1 = box
    sub = subject.crop(box)
    target = TILE * (0.72 * zoom)
    scale = target / max(sub.size)
    sub = sub.resize((max(1, int(sub.size[0] * scale)), max(1, int(sub.size[1] * scale))),
                     Image.LANCZOS)

    canvas = Image.new("RGBA", (CANVAS, CANVAS), (0, 0, 0, 0))
    canvas.paste(tile, (0, 0))

    ox = (CANVAS - sub.size[0]) // 2
    oy = (CANVAS - sub.size[1]) // 2
    # A soft shadow under the subject, so it sits on the tile rather than
    # floating flat against it.
    drop = Image.new("RGBA", (CANVAS, CANVAS), (0, 0, 0, 0))
    silhouette = Image.new("RGBA", sub.size, (0, 0, 0, 110))
    drop.paste(silhouette, (ox, oy + int(CANVAS * 0.018)), sub.split()[3])
    drop = drop.filter(ImageFilter.GaussianBlur(CANVAS * 0.016))
    canvas.alpha_composite(drop)
    canvas.alpha_composite(sub.convert("RGBA"), (ox, oy))
    canvas.putalpha(tile_mask)

    # Contact shadow, sitting just below the tile like Apple's template.
    out = Image.new("RGBA", (CANVAS, CANVAS), (0, 0, 0, 0))
    shadow = Image.new("RGBA", (CANVAS, CANVAS), (0, 0, 0, 0))
    shadow.paste((0, 0, 0, 90), (0, 0), tile_mask)
    shadow = shadow.filter(ImageFilter.GaussianBlur(14))
    out.alpha_composite(shadow, (0, 12))
    out.alpha_composite(canvas)
    return out.resize((px, px), Image.LANCZOS)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("source")
    ap.add_argument("--out", default="src-tauri/icons")
    args = ap.parse_args()

    art = Image.open(args.source).convert("RGB")
    if art.size != (CANVAS, CANVAS):
        art = art.resize((CANVAS, CANVAS), Image.LANCZOS)
    subject, box = cut_subject(art)
    grad = gradient(GRAD_TOP, GRAD_BOTTOM)
    print(f"subject box {box}  gradient {GRAD_TOP} -> {GRAD_BOTTOM}")

    out = pathlib.Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    iconset = out / "icon.iconset"
    if iconset.exists():
        shutil.rmtree(iconset)
    iconset.mkdir()

    rendered = {}
    for px, names in ICONSET:
        # Crop in at the sizes where the whole shelf would be unreadable.
        zoom = 1.30 if px <= 32 else (1.12 if px <= 64 else 1.0)
        im = build(subject, box, px, zoom, grad)
        rendered[px] = im
        for n in names:
            im.save(iconset / n)
        print(f"  {px:>4}px  zoom {zoom:>4}  -> {', '.join(names)}")

    icns = out / "icon.icns"
    subprocess.run(["iconutil", "-c", "icns", str(iconset), "-o", str(icns)], check=True)
    print(f"\n{icns}  {icns.stat().st_size/1024:.0f} KB")

    # The PNGs Tauri lists alongside the .icns, plus the Windows/Store set.
    # Only what tauri.conf.json actually references, plus the 1024 master.
    # The Square*/Store logos and the android/ios trees Tauri scaffolds are
    # unused here and were deleted.
    rendered[1024].save(out / "icon.png")
    rendered[32].save(out / "32x32.png")
    rendered[128].save(out / "128x128.png")
    rendered[256].save(out / "128x128@2x.png")

    # PIL builds an .ico by downsampling the image it is given, so it has to be
    # handed the largest one — saving from the 16px frame yields a 16-only file.
    rendered[256].save(out / "icon.ico",
                       sizes=[(s, s) for s in (16, 32, 48, 64, 128, 256)])
    print(f"{out/'icon.ico'}  {(out/'icon.ico').stat().st_size/1024:.0f} KB")
    return 0


if __name__ == "__main__":
    sys.exit(main())
