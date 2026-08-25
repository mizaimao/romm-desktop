#!/bin/bash
#
# RomM, as a KNULLI port.
#
# EmulationStation runs this and stands aside while it lasts.

# The working directory matters. The app looks for `data/esde-core-map.json`
# and its own `romm-sdl.toml` relative to where it was started, so a launcher
# that does not cd finds neither: an empty library and default settings, with
# nothing on screen to say why.
cd /userdata/system/romm || exit 1

# Everything this prints, kept.
#
# Launched from the Ports menu there is no terminal to print to, so without
# this a failure is a screen that goes black and comes back with no evidence
# anywhere. `/userdata` is the writable partition and this sits beside the
# binary, where it can be found without knowing anything.
exec > >(tee ./log.txt) 2>&1
echo "--- $(date) ---"

# Straight to the display. This device has no compositor and no X server, so
# SDL trying wayland first is a few seconds of failing before it gets to the
# driver that was always going to work. Overridable, because a device that
# does have one should say so rather than be argued with.
export SDL_VIDEODRIVER="${SDL_VIDEODRIVER:-kmsdrm}"

./romm-sdl "$@"
status=$?
echo "--- exit $status ---"

# Leave the console as it was found. Without this the framebuffer keeps the
# last frame drawn on it, which looks like the app never closed.
printf "\033c" > /dev/tty0 2>/dev/null
exit $status
