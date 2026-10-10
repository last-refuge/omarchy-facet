#!/usr/bin/env bash
# Launch facet-app with the given args, screenshot its window by PID, then kill it.
# usage: scripts/shot.sh <out.png> [facet-app args...]    (env: WAIT=seconds, default 3)
set -u
out=$1; shift
cd "$(dirname "$0")/.."
if ps -eo pid,ppid,stat,comm | awk '$3 ~ /D/ && $2 != 2 && $1 != 2 {f=1} END {exit !f}'; then
  echo "stuck D-state processes present; refusing to launch a GUI" >&2; exit 3
fi
XDG_STATE_HOME="${SHOT_STATE:-/tmp/facet-shot-state}" timeout -s KILL 30 ./target/release/facet-app "$@" >/dev/null 2>&1 &
sleep "${WAIT:-2.5}"
pid=$(pgrep -n -x facet-app)
# Omarchy makes every window slightly translucent, which would bleed other windows' content into the
# capture. Force only this window opaque (runtime property of our own window; no config is changed).
addr=$(hyprctl clients -j | python3 -c 'import json,sys
for c in json.load(sys.stdin):
    if c["pid"]==int(sys.argv[1]): print(c["address"])' "$pid")
[ -n "$addr" ] && hyprctl dispatch setprop "address:$addr" opaque 1 >/dev/null 2>&1 && sleep 0.4
geo=$(hyprctl clients -j | python3 -c 'import json,sys
for c in json.load(sys.stdin):
    if c["pid"]==int(sys.argv[1]): print("%d,%d %dx%d"%(*c["at"],*c["size"]))' "$pid")
if [ -z "$geo" ]; then echo "window not found" >&2; kill -KILL "$pid" 2>/dev/null; exit 1; fi
grim -g "$geo" "$out"
kill -KILL "$pid" 2>/dev/null
echo "$out ($geo)"
