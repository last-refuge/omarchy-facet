#!/usr/bin/env bash
# Regression test for `curl | bash`: a command that reads stdin (like pacman's "Proceed? [Y/n]") must not
# swallow the rest of the installer. Uses stub curl/sudo/omarchy in a fake HOME; touches nothing real.
set -euo pipefail
cd "$(dirname "$0")/.."
T=$(mktemp -d /tmp/facet-installer-test.XXXX); trap 'rm -rf "$T"' EXIT
mkdir -p "$T/stubs" "$T/home/.config/omarchy/plugins"
cat > "$T/stubs/curl" <<'STUB'
#!/bin/bash
out=""; url=""; while [ $# -gt 0 ]; do case "$1" in -o) out=$2; shift;; http*) url=$1;; esac; shift; done
case "$url" in *.sha256) (cd "$(dirname "$out")" && sha256sum facet-x86_64.pkg.tar.zst > "$out");; *) echo fakepkg > "$out";; esac
STUB
printf '#!/bin/bash\ncat >/dev/null\necho "(stub sudo ran)"\n' > "$T/stubs/sudo"   # swallows stdin like pacman
printf '#!/bin/sh\nexit 0\n' > "$T/stubs/omarchy"; cp "$T/stubs/omarchy" "$T/stubs/omarchy-shell"; chmod +x "$T"/stubs/*
out=$(cat install.sh | PATH="$T/stubs:$PATH" HOME="$T/home" bash -s -- --yes 2>&1) || { echo "FAIL: installer exited non-zero"; echo "$out"; exit 1; }
grep -q "Installed" <<<"$out"        || { echo "FAIL: never reached the install step"; echo "$out"; exit 1; }
grep -q "Checked: facet" <<<"$out"   || { echo "FAIL: stdin was swallowed; the installer stopped after the install step"; echo "$out"; exit 1; }
grep -q "Remove it any time" <<<"$out" || { echo "FAIL: closing block missing"; exit 1; }
! grep -q "unexpected EOF" <<<"$out" || { echo "FAIL: syntax error from a truncated script"; exit 1; }
echo "ok  installer survives a stdin-reading command under curl | bash"
