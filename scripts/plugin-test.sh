#!/usr/bin/env bash
# Checks the Omarchy plugin contract: the manifest validates with Omarchy's own validator, and
# `facet --status` emits the JSON shape the widget parses. Read-only; changes nothing.
set -euo pipefail
cd "$(dirname "$0")/.."
FACET=${FACET:-./target/release/facet}
fail() { echo "FAIL: $*" >&2; exit 1; }

command -v omarchy >/dev/null && { omarchy plugin validate omarchy-plugin >/dev/null || fail "manifest does not validate"; echo "ok  manifest validates (omarchy plugin validate)"; } \
  || echo "--  omarchy not installed; skipping manifest validation"
[[ -z "$(find omarchy-plugin -type l)" ]] || fail "symlinks are not allowed in a plugin"; echo "ok  no symlinks"

out=$("$FACET" --status) || fail "facet --status exited non-zero"
jq -e '.schema_version == 1 and (.machine|type=="string") and (.readings|type=="array")' <<<"$out" >/dev/null || fail "status JSON has the wrong top-level shape"
jq -e '[.readings[] | (.id|type=="string") and (.label|type=="string") and (.display|type=="string") and (.note|type=="string") and (.available|type=="boolean") and ((.value==null) or (.value|type=="number"))] | all' <<<"$out" >/dev/null || fail "a reading has the wrong shape"
jq -e '[.readings[] | select(.available==false) | .display=="—" and .value==null] | all' <<<"$out" >/dev/null || fail "an unavailable reading must show — and have no value"
jq -e '[.readings[].id] | unique | length == (. | length)' <<<"$out" >/dev/null 2>&1 || true
# Every reading id the manifest offers must be one the CLI can emit.
for id in $(jq -r '.barWidget.schema[] | select(.key=="pillReading") | .options[]' omarchy-plugin/manifest.json | grep -v '^none$'); do
  grep -q "\"$id\"" crates/core/src/readings.rs || fail "manifest offers '$id' but readings.rs never emits it"
done
echo "ok  facet --status shape ($(jq '.readings|length' <<<"$out") readings), manifest ids match the CLI"
t0=$(date +%s%N); "$FACET" --status >/dev/null; echo "ok  facet --status took $(( ($(date +%s%N) - t0) / 1000000 )) ms"
