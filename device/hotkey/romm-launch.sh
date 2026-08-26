#!/bin/sh
# Fired by triggerhappy on L2+R2. triggerhappy reads the pad below both ES
# and RetroArch, so this fires inside games too -- where L2 and R2 are
# ordinary game buttons. Do nothing unless we are back in the menu.
LOG=/userdata/system/logs/romm-launch.log
APP=/userdata/system/romm/romm-sync
STAMP=/var/run/romm-launch.stamp

# Both orderings of the combo are bound, so without this it fires twice.
now=$(cut -d' ' -f1 /proc/uptime)
last=$(cat "$STAMP" 2>/dev/null || echo 0)
[ "$(awk -v n="$now" -v l="$last" 'BEGIN{print (n-l < 2)}')" = "1" ] && exit 0
echo "$now" > "$STAMP"

if pgrep -x retroarch >/dev/null 2>&1 || pgrep -f emulatorlauncher >/dev/null 2>&1; then
  echo "$(date '+%F %T') L2+R2 ignored, a game is running" >> "$LOG"
  exit 0
fi

echo "$(date '+%F %T') L2+R2 fired" >> "$LOG"
if [ -x "$APP" ]; then
  "$APP" >> "$LOG" 2>&1
else
  echo "  no sync app at $APP yet" >> "$LOG"
fi
