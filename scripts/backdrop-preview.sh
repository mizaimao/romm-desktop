#!/usr/bin/env bash
# Serve the backdrop preview.
#
# The page imports ui/js/backdrop.js as an ES module, and a browser refuses a
# module import over file:// — it is a cross-origin request from a null origin,
# so opening the html by double-clicking it gets a blank screen and a CORS
# message. Hence a server. It only ever hands over files; nothing here builds,
# watches or rewrites anything.
set -euo pipefail
cd "$(dirname "$0")/.."

port="${1:-8765}"
while lsof -i ":$port" >/dev/null 2>&1; do port=$((port + 1)); done

url="http://localhost:$port/tools/backdrop-preview.html"
printf '\n  %s\n\n  Leave this running. Editing ui/js/backdrop.js updates the page on\n  its own within a second — no refresh, no build.\n  Ctrl-C to stop.\n\n' "$url"
exec python3 -m http.server "$port" --bind 127.0.0.1
