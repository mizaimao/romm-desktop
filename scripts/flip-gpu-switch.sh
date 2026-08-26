#!/usr/bin/env bash
#
# Install two entries under Ports on the Flip for switching the GPU driver.
#
# The device ships `libmali-bifrost-g52-g13p0-gbm` — no Wayland, old GBM — and
# that is why no compositor starts. `g24p0-wayland-gbm` is the same vendor
# driver built with both. Emulators measured identical on either: same frame
# rate, byte-identical rendered frames. See
# docs/flip-wayland-and-the-gpu-blob.md.
#
#   ./scripts/flip-gpu-switch.sh install    put the two entries under Ports
#   ./scripts/flip-gpu-switch.sh remove     take them off, restore stock
#
# Unlike the /tmp shim this is a *persistent* change to /usr/lib, which is a
# writable overlay here. The stock driver is kept beside the new one and either
# entry restores the other, but a driver that will not load means a black screen
# fixable only over ssh. That is why the on-device script keeps a backup and
# checks it before swapping.
set -euo pipefail

FLIP="${FLIP:-10.10.10.187}"
FLIP_PASSWORD="${FLIP_PASSWORD:-linux}"
GPU=/userdata/system/gpu
PORTS=/userdata/roms/ports
BLOB=libmali-bifrost-g52-g24p0-wayland-gbm.so
URL="https://raw.githubusercontent.com/ROCKNIX/libmali/master/lib/aarch64-linux-gnu/$BLOB"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CACHE="$HERE/.toolchain/$BLOB"

say() { printf '\033[1m==>\033[0m %s\n' "$*"; }

ssh_do() {
  expect -c "
    set timeout 900
    log_user 0
    spawn ssh -tt -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null root@$FLIP {$1}
    expect \"assword:\" { send \"$FLIP_PASSWORD\r\" }
    log_user 1
    expect eof
  "
}
send() {
  expect -c "
    set timeout 1800
    log_user 0
    spawn scp -O -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null $1 root@$FLIP:$2
    expect \"assword:\" { send \"$FLIP_PASSWORD\r\" }
    expect eof" >/dev/null
}

# The script that does the work, written out here so it lives in the repo
# rather than only on a device.
switcher() {
cat <<'SWITCH'
#!/bin/sh
# Switch the Mali userspace driver. Run from Ports; takes effect on reboot.
#
# Only /usr/lib/libmali.so.1 is replaced. Everything that matters resolves
# through it — libEGL.so.1, libGLESv2.so.2, libgbm.so.1 are all symlinks to
# that one file — so one swap moves the whole stack together, which is the
# thing that must not be done by halves.
GPU=/userdata/system/gpu
LIVE=/usr/lib/libmali.so.1
say() { echo "$*"; echo "$*" > /dev/tty0 2>/dev/null; }

case "$1" in
  wayland) WANT=$GPU/libmali-g24p0-wayland.so ; NAME="g24p0 (Wayland)" ;;
  stock)   WANT=$GPU/libmali-g13p0-stock.so   ; NAME="g13p0 (stock)"   ;;
  *) echo "usage: $0 wayland|stock" ; exit 1 ;;
esac

if [ ! -s "$WANT" ]; then
  say "GPU: $NAME is not on this device — nothing changed."
  sleep 4; exit 1
fi

# Never proceed without a way back.
if [ ! -s "$GPU/libmali-g13p0-stock.so" ]; then
  say "GPU: no backup of the stock driver — refusing to switch."
  sleep 4; exit 1
fi

if cmp -s "$WANT" "$LIVE"; then
  say "GPU: already using $NAME. Nothing to do."
  sleep 3; exit 0
fi

say "GPU: switching to $NAME..."
cp "$WANT" "$LIVE.new" || { say "GPU: copy failed, nothing changed."; sleep 4; exit 1; }
mv "$LIVE.new" "$LIVE" || { say "GPU: swap failed, nothing changed."; sleep 4; exit 1; }
say "GPU: now $NAME. Reboot for everything to pick it up."
sleep 5
SWITCH
}

case "${1:-}" in
  install)
    if [ ! -s "$CACHE" ]; then
      say "fetching the Wayland-capable driver (56 MB)"
      mkdir -p "$(dirname "$CACHE")"
      curl -fL --progress-bar "$URL" -o "$CACHE"
    fi
    say "making $GPU and backing up the driver the device is running"
    ssh_do "mkdir -p $GPU && [ -s $GPU/libmali-g13p0-stock.so ] || cp /usr/lib/libmali.so.1 $GPU/libmali-g13p0-stock.so; ls -la $GPU" >/dev/null
    say "sending the Wayland driver"
    send "$CACHE" "$GPU/libmali-g24p0-wayland.so"

    say "installing the switcher and its two Ports entries"
    switcher > /tmp/gpu-switch.sh
    send /tmp/gpu-switch.sh "$GPU/gpu-switch.sh"
    rm -f /tmp/gpu-switch.sh
    ssh_do "chmod +x $GPU/gpu-switch.sh; \
      printf '#!/bin/bash\n$GPU/gpu-switch.sh wayland\n' > '$PORTS/GPU driver - Wayland.sh'; \
      printf '#!/bin/bash\n$GPU/gpu-switch.sh stock\n' > '$PORTS/GPU driver - stock.sh'; \
      chmod +x '$PORTS/GPU driver - Wayland.sh' '$PORTS/GPU driver - stock.sh'; \
      ls -la '$PORTS' | grep GPU" 
    say "done — two entries under Ports. They take effect on reboot."
    ;;
  remove)
    say "restoring the stock driver and removing the entries"
    ssh_do "[ -s $GPU/libmali-g13p0-stock.so ] && cp $GPU/libmali-g13p0-stock.so /usr/lib/libmali.so.1 && echo 'stock driver restored'; \
      rm -f '$PORTS/GPU driver - Wayland.sh' '$PORTS/GPU driver - stock.sh'; rm -rf $GPU; echo removed"
    ;;
  status)
    ssh_do "if cmp -s $GPU/libmali-g24p0-wayland.so /usr/lib/libmali.so.1 2>/dev/null; then echo 'running: g24p0 (Wayland)'; else echo 'running: g13p0 (stock)'; fi; ls $PORTS 2>/dev/null | grep -c GPU"
    ;;
  *) sed -n '3,20p' "$0" | sed 's/^# \{0,1\}//'; exit 1 ;;
esac
