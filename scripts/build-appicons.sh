#!/usr/bin/env bash
# Builds every app icon in assets/appicons/ into the shapes each OS wants, and
# copies the default one into src-tauri/icons/ where the bundler looks.
#
# Adding an icon is dropping a 1024x1024 PNG into assets/appicons/ and naming it
# in src/appicon.rs. Nothing here is per-icon.
#
# macOS masks nothing: whatever is in the .icns is drawn as-is, so a full-bleed
# square sits in a Dock of rounded squares looking like a mistake. The rounded
# corner is therefore cut here, into the .icns only — Windows and Linux draw
# the square themselves.
set -euo pipefail
cd "$(dirname "$0")/.."

DEFAULT="${1:-arcade}"
SRC=assets/appicons
OUT=assets/appicons/built

command -v magick >/dev/null || { echo "needs ImageMagick (brew install imagemagick)"; exit 1; }

# The macOS corner radius, as a share of the icon's side. Apple's own icons sit
# at just under 22.4%; the value is not published, it is measured.
RADIUS_PCT=22.37

rm -rf "$OUT"
mkdir -p "$OUT"

for src in "$SRC"/*.png; do
  id=$(basename "$src" .png)
  work="$OUT/$id"
  mkdir -p "$work"

  # Square, 1024, whatever came in.
  # PNG32: forces RGBA. Tauri's bundler refuses any icon that is not RGBA,
  # and a plain resize of an opaque JPEG-ish PNG comes out RGB.
  magick "$src" -resize 1024x1024^ -gravity center -extent 1024x1024 PNG32:"$work/full.png"

  # The rounded-corner copy the .icns is cut from.
  r=$(python3 -c "print(round(1024 * $RADIUS_PCT / 100))")
  # -fill white matters: the default fill is black, which composites to a
  # fully transparent icon and looks like the build silently produced nothing.
  magick -size 1024x1024 xc:black -fill white \
    -draw "roundrectangle 0,0,1023,1023,$r,$r" "$work/mask.png"
  magick "$work/full.png" "$work/mask.png" -alpha off -compose CopyOpacity -composite PNG32:"$work/rounded.png"

  # macOS wants ten sizes in an .iconset; iconutil refuses a folder missing any.
  iconset="$work/$id.iconset"
  mkdir -p "$iconset"
  for s in 16 32 128 256 512; do
    magick "$work/rounded.png" -resize "${s}x${s}"     PNG32:"$iconset/icon_${s}x${s}.png"
    magick "$work/rounded.png" -resize "$((s*2))x$((s*2))" PNG32:"$iconset/icon_${s}x${s}@2x.png"
  done
  if command -v iconutil >/dev/null; then
    iconutil -c icns "$iconset" -o "$work/$id.icns"
  else
    echo "no iconutil (not macOS) — skipping $id.icns"
  fi

  # Windows and Linux: the square, unrounded.
  magick "$work/full.png" -define icon:auto-resize=256,128,64,48,32,16 "$work/$id.ico"
  for s in 32 128 256 512; do
    magick "$work/full.png" -resize "${s}x${s}" PNG32:"$work/${s}x${s}.png"
  done
  magick "$work/full.png" -resize 256x256 PNG32:"$work/128x128@2x.png"

  # The preview the Settings picker draws.
  magick "$work/rounded.png" -resize 256x256 PNG32:"$work/preview.png"

  rm -f "$work/mask.png"
  echo "built $id"
done

# The default is what the bundler compiles in, so a fresh install starts there.
d="$OUT/$DEFAULT"
[ -d "$d" ] || { echo "no such icon: $DEFAULT"; exit 1; }
cp "$d/$DEFAULT.icns" src-tauri/icons/icon.icns
cp "$d/$DEFAULT.ico"  src-tauri/icons/icon.ico
cp "$d/full.png"      src-tauri/icons/icon.png
cp "$d/32x32.png"     src-tauri/icons/32x32.png
cp "$d/128x128.png"   src-tauri/icons/128x128.png
cp "$d/128x128@2x.png" src-tauri/icons/128x128@2x.png
rm -rf src-tauri/icons/icon.iconset src-tauri/icons/macos.iconset
cp -R "$d/$DEFAULT.iconset" src-tauri/icons/icon.iconset

# Everything else was scaffolding for the two files above. The bundler globs
# this folder into the app, so what is left here is what every user downloads:
# the macOS icon to swap in, the square PNG the window icon is set from, and
# the picture the picker draws. Nothing else earns its bytes.
for work in "$OUT"/*/; do
  id=$(basename "$work")
  find "$work" -mindepth 1 \
    ! -name "$id.icns" ! -name "256x256.png" ! -name "preview.png" \
    -delete 2>/dev/null || true
done

echo "default is $DEFAULT"
