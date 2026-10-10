#!/usr/bin/env bash
# Launch each screen against a fixture (or live with FIXTURE=live) and audit its AT-SPI tree.
set -u
cd "$(dirname "$0")/.."
fx=${FIXTURE:-laptop}
args=(--fixture "$fx"); [ "$fx" = live ] && args=()
status=0
for page in overview category:storage category:peripherals component:storage:nvme0n1 connections preferences; do
  for extra in "" "--overlay search" "--overlay export"; do
    [ "$extra" != "" ] && [ "$page" != overview ] && continue
    XDG_STATE_HOME=/tmp/facet-shot-state timeout -s KILL 12 ./target/release/facet-app "${args[@]}" --page "$page" --theme light $extra >/dev/null 2>&1 &
    sleep 2.2
    out=$(timeout 10 python3 scripts/a11y-audit.py facet 2>&1)
    pkill -KILL -x facet-app; wait 2>/dev/null
    printf '%-34s %s\n' "$page ${extra}" "$out"
    case "$out" in ok*) ;; *) status=1;; esac
  done
done
exit $status
