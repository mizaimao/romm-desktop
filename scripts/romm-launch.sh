#!/bin/bash
#
# RomM, as a KNULLI port.
#
# EmulationStation runs this and *waits* on it. That word is the whole reason
# this file is careful: while it waits, ES is not drawing, and what is on the
# screen is whatever was in the framebuffer before — which on this device is
# the boot logo `fbv` painted there at startup. A launcher that does not return
# leaves you looking at that logo with no way back.

# The working directory matters. The app looks for `data/esde-core-map.json`
# and its own `romm-sdl.toml` relative to where it was started, so a launcher
# that does not cd finds neither: an empty library and default settings, with
# nothing on screen to say why.
cd /userdata/system/romm || exit 1

# Plain redirection, not `tee`.
#
# It used to be `exec > >(tee ./log.txt)`, and process substitution starts a
# `tee` that inherits the script's standard output and outlives the app. The
# pipe stays open, so whatever is waiting on this script goes on waiting after
# the app has gone — which is exactly "quit the app and the device sits on the
# KNULLI logo". The log is worth having; a second process holding the door open
# is not.
exec >./log.txt 2>&1
echo "--- $(date) ---"

# Straight to the display. This device has no compositor and no X server, so
# SDL trying wayland first is a few seconds of failing before it gets to the
# driver that was always going to work. Overridable, because a device that
# does have one should say so rather than be argued with.
export SDL_VIDEODRIVER="${SDL_VIDEODRIVER:-kmsdrm}"

./romm-sdl "$@"
status=$?
echo "--- exit $status ---"

# Wipe what was on the console before handing it back.
#
# Two different things are on there. `fbv` painted the boot logo straight into
# /dev/fb0 at startup, which no amount of clearing the *text* console removes —
# so the framebuffer is zeroed — and the terminal itself is reset, or the
# cursor and any text it holds show through whatever draws next.
cat /dev/zero > /dev/fb0 2>/dev/null
printf "\033c" > /dev/tty0 2>/dev/null
exit $status
