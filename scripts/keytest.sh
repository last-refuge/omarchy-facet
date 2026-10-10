#!/usr/bin/env bash
# Drive facet-app with real key events (wtype) and read the focused element from AT-SPI.
# Every send is guarded: it aborts unless the ACTIVE window is the facet-app we launched.
set -u
cd "$(dirname "$0")/.."
args=("$@")
XDG_STATE_HOME=/tmp/facet-shot-state timeout -s KILL 60 ./target/release/facet-app "${args[@]}" >/dev/null 2>&1 &
sleep 2.5
PID=$(pgrep -n -x facet-app)
active_ok() { [ "$(hyprctl activewindow -j | python3 -c 'import json,sys; print(json.load(sys.stdin).get("pid",0))')" = "$PID" ]; }
key() {  # key <wtype args...>
  if ! active_ok; then echo "ABORT: facet-app is not the active window; refusing to send keys" >&2; pkill -KILL -x facet-app; exit 4; fi
  wtype "$@"; sleep 0.35
}
focus() { timeout 8 python3 scripts/a11y-focus.py; }
echo "start:        $(focus)"
for i in $(seq 1 ${TABS:-14}); do key -k Tab; printf 'Tab %-2s ->     %s\n' $i "$(focus)"; done
pkill -KILL -x facet-app
