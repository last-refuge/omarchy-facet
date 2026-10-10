#!/usr/bin/env bash
# 1) Keyboard-copy: open a device, Tab to a value row, press Enter, read the clipboard with wl-paste.
# 2) Persistence: navigate, quit, relaunch without --page and confirm the page was remembered.
set -u
cd "$(dirname "$0")/.."
STATE=$(mktemp -d /tmp/facet-persist-XXXX)
launch() { XDG_STATE_HOME=$STATE timeout -s KILL 40 ./target/release/facet-app --fixture laptop --theme light "$@" >/dev/null 2>&1 & sleep 2.5; PID=$(pgrep -n -x facet-app); }
active_ok() { [ "$(hyprctl activewindow -j | python3 -c 'import json,sys; print(json.load(sys.stdin).get("pid",0))')" = "$PID" ]; }
key() { if ! active_ok; then echo "ABORT: facet-app not active" >&2; pkill -KILL -x facet-app; exit 4; fi; wtype "$@"; sleep 0.45; }
echo -n "" | wl-copy 2>/dev/null
launch --page component:storage:nvme0n1
for i in $(seq 1 16); do key -k Tab; f=$(timeout 8 python3 scripts/a11y-focus.py); case "$f" in *"Firmware: "*) break;; esac; done
echo "focused: $f"
key -k Return
echo "clipboard after Enter: '$(wl-paste 2>/dev/null)'"
pkill -KILL -x facet-app; sleep 0.5
# persistence: go to Storage via the keyboard, quit, relaunch
launch --page overview
for i in $(seq 1 6); do key -k Tab; done; key -k Return
echo "after nav:   $(timeout 8 python3 scripts/a11y-page.py)"
pkill -KILL -x facet-app; sleep 0.5
launch
echo "relaunched:  $(timeout 8 python3 scripts/a11y-page.py)   (state: $(cat $STATE/facet/state.json))"
pkill -KILL -x facet-app; rm -rf "$STATE"
