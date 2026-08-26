#!/bin/sh
#
# L2+R2, from triggerhappy.
#
# triggerhappy reads the pads directly, below both EmulationStation and
# whatever emulator is running, so this fires everywhere — including inside a
# game, where L2 and R2 are just buttons. The first thing it does is decide
# whether it should be running at all.
#
# The second thing it has to deal with is DRM. EmulationStation holds the
# display; it only lets go because *it* launches the emulator and drops master
# for the duration. Nothing launched from outside gets that courtesy, so the
# app would open on a device whose screen belongs to someone else and fail.
# Stopping ES is the honest way to get the display, and the init script's
# `start` is the only way to get it back with the environment it needs —
# started by hand it comes up without XDG_RUNTIME_DIR and has no sound.
LOG=/userdata/system/logs/romm-hotkey.log
ROMM=/userdata/system/romm
STAMP=/var/run/romm-hotkey.stamp

log() { echo "$(date '+%F %T') $*" >>"$LOG"; }

# Both orderings of the combo are bound, so without this it fires twice.
now=$(cut -d' ' -f1 /proc/uptime)
last=$(cat "$STAMP" 2>/dev/null || echo 0)
[ "$(awk -v n="$now" -v l="$last" 'BEGIN{print (n-l < 3)}')" = "1" ] && exit 0
echo "$now" >"$STAMP"

# A game owns the screen: those two buttons are the game's, not ours.
if pgrep -x retroarch >/dev/null 2>&1 || pgrep -f emulatorlauncher >/dev/null 2>&1; then
  log "ignored, a game is running"
  exit 0
fi

# Nothing to launch yet is not an error, it is just early.
if [ ! -x "$ROMM/romm-sdl" ]; then
  log "no app at $ROMM/romm-sdl"
  exit 0
fi

# Whatever happens below, EmulationStation comes back. Without this a crash in
# the app leaves the device on a black screen with no way to the menu.
restore() { /etc/init.d/S31emulationstation start >/dev/null 2>&1; }
trap restore EXIT INT TERM

log "L2+R2, stopping EmulationStation"
/etc/init.d/S31emulationstation stop >/dev/null 2>&1
i=0
while [ $i -lt 20 ]; do
  ps -e -o args | grep -q '^emulationstation ' || break
  i=$((i + 1)); sleep 1
done

log "running the app"
"$ROMM/romm-launch.sh"
log "app exited $?, restoring EmulationStation"
