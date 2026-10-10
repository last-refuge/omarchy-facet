#!/usr/bin/env bash
# Keyboard-only workflow test with the active-window guard (see keytest.sh).
set -u
cd "$(dirname "$0")/.."
XDG_STATE_HOME=/tmp/facet-shot-state timeout -s KILL 60 ./target/release/facet-app --fixture laptop --page overview --theme light >/dev/null 2>&1 &
sleep 2.5
PID=$(pgrep -n -x facet-app)
active_ok() { [ "$(hyprctl activewindow -j | python3 -c 'import json,sys; print(json.load(sys.stdin).get("pid",0))')" = "$PID" ]; }
key() { if ! active_ok; then echo "ABORT: facet-app not active" >&2; pkill -KILL -x facet-app; exit 4; fi; wtype "$@"; sleep 0.5; }
show() { printf '%-34s %s | focus: %s\n' "$1" "$(timeout 8 python3 scripts/a11y-page.py)" "$(timeout 8 python3 scripts/a11y-focus.py)"; }
show "start"
key -M ctrl -k k -m ctrl;            show "Ctrl+K (open search)"
key "nvme";                          show "type 'nvme'"
key -k Return;                       show "Enter (open result)"
key -M alt -k Left -m alt;           show "Alt+Left (back)"
key -k Escape;                       show "Esc (nothing open)"
key -M ctrl -k e -m ctrl;            show "Ctrl+E (export dialog)"
key -k Tab; key -k Tab; key -k Tab;  show "Tab x3 inside export dialog"
key -M shift -k Tab -m shift;        show "Shift+Tab inside dialog"
key -k Escape;                       show "Esc (close export)"
for i in 1 2 3 4 5 6; do key -k Tab; done; show "Tab x6 (-> Storage nav)"
key -k Return;                       show "Enter on Storage nav"
key -M shift -k Tab -m shift;        show "Shift+Tab"
pkill -KILL -x facet-app
