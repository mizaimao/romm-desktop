#!/bin/bash
# RomM, as a KNULLI port.
#
# The working directory matters: the app looks for `data/esde-core-map.json`
# and its own `romm-sdl.toml` relative to where it was started, so a launcher
# that does not cd finds neither and comes up with an empty library and default
# settings.
cd /userdata/system/romm || exit 1
exec ./romm-sdl "$@"
