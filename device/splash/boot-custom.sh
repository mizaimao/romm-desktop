#!/bin/bash
# Things that have to be put back at every boot.
#
# The writable layer of / is a tmpfs, so everything under /usr is the stock
# image again on every start. Whatever we want instead lives in /userdata,
# which is real storage. This runs as S00bootcustom, before the splash (S03)
# and before EmulationStation (S31).

# The chosen Mali driver.
apply_gpu() {
  GPU=/userdata/system/gpu
  [ -s "$GPU/selected" ] || return 0
  case "$(tr -d '\r\n' <"$GPU/selected" 2>/dev/null)" in
    wayland) WANT=$GPU/libmali-g24p0-wayland.so ;;
    stock)   WANT=$GPU/libmali-g13p0-stock.so ;;
    *) return 0 ;;
  esac
  [ -s "$WANT" ] || return 0
  cp "$WANT" /usr/lib/libmali.so.1.new 2>/dev/null && \
    mv /usr/lib/libmali.so.1.new /usr/lib/libmali.so.1 2>/dev/null
}

# The KNULLI logo EmulationStation draws when it is loading — which is every
# game launch and every return from one. It is not a setting: ES draws
# resources/logo.png, so the only way to not see it is for that file to have
# nothing in it. Must happen before S31 starts ES.
blank_es_logo() {
  BLANK=/userdata/system/romm/blank-logo.png
  DEST=/usr/share/emulationstation/resources/logo.png
  [ -s "$BLANK" ] || return 0
  [ -e "$DEST" ] || return 0
  cp "$BLANK" "$DEST" 2>/dev/null
}

case "$1" in
  start)
    apply_gpu
    blank_es_logo
    ;;
esac
exit 0
