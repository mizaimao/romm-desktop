#!/bin/bash
#
# moose-patch on the handheld. One file, two ways in:
#
#   moose-launch.sh            EmulationStation started us, as a Port. ES has
#                              already dropped the display and is waiting on us.
#   moose-launch.sh --hotkey   triggerhappy started us, on L2+R2. Nobody
#                              dropped anything, so we stop ES ourselves.
#
# The difference is only who owns the screen. ES holds DRM master and lets go
# for children it launches itself; nothing started from outside gets that.

MOOSE=/userdata/system/moose-patch
LOG=/userdata/system/logs/moose-patch.log
STAMP=/var/run/moose-patch.stamp
HOTKEY=0
[ "${1:-}" = "--hotkey" ] && { HOTKEY=1; shift; }

log() { echo "$(date '+%F %T') $*" >>"$LOG"; }

if [ "$HOTKEY" = 1 ]; then
  # Both orderings of the combo are bound, so without this it fires twice.
  now=$(cut -d' ' -f1 /proc/uptime)
  last=$(cat "$STAMP" 2>/dev/null || echo 0)
  [ "$(awk -v n="$now" -v l="$last" 'BEGIN{print (n-l < 3)}')" = "1" ] && exit 0
  echo "$now" >"$STAMP"

  # A game owns the screen: L2 and R2 are the game's buttons, not ours.
  if pgrep -x retroarch >/dev/null 2>&1 || pgrep -f emulatorlauncher >/dev/null 2>&1; then
    log "L2+R2 ignored, a game is running"
    exit 0
  fi
  [ -x "$MOOSE/moose-patch" ] || { log "no app at $MOOSE/moose-patch"; exit 0; }

  # Whatever happens below, ES comes back. Through the init script, because
  # started by hand it comes up without XDG_RUNTIME_DIR and has no sound — and
  # with setsid, because the init script backgrounds it from *this* shell and
  # SIGHUP would take it with us when we exit.
  restore() {
    . /etc/profile.d/xdg.sh 2>/dev/null
    . /etc/profile.d/dbus.sh 2>/dev/null
    setsid /usr/bin/emulationstation-standalone </dev/null >/dev/null 2>&1 &
  }
  trap restore EXIT INT TERM

  log "L2+R2, stopping EmulationStation"
  /etc/init.d/S31emulationstation stop >/dev/null 2>&1
  i=0
  while [ $i -lt 20 ]; do
    ps -e -o args= | grep -q '^emulationstation ' || break
    i=$((i + 1)); sleep 1
  done
fi

cd "$MOOSE" || exit 1

# Straight to the display. No compositor and no X server here, so SDL trying
# wayland first is a few seconds of failing before it reaches the driver that
# was always going to work.
export SDL_VIDEODRIVER="${SDL_VIDEODRIVER:-kmsdrm}"

./moose-patch "$@" >>"$LOG" 2>&1
status=$?
log "moose-patch exited $status"

# Hand the console back clean, or the cursor and whatever text it holds show
# through whatever draws next.
printf "\033c" >/dev/tty0 2>/dev/null
exit $status
