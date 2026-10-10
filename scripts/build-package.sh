#!/usr/bin/env bash
# Build the Arch package from the committed tree, in ./target/package. Never installs anything.
set -euo pipefail
cd "$(dirname "$0")/.."
ver=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
out=target/package
rm -rf "$out"; mkdir -p "$out/src"
git archive --format=tar.gz --prefix="facet-$ver/" -o "$out/facet-$ver.tar.gz" HEAD
sed -e "s|^source=.*|source=(\"facet-\$pkgver.tar.gz\")|" -e "s|^sha256sums=.*|sha256sums=('SKIP')|" packaging/PKGBUILD > "$out/PKGBUILD"
cd "$out"
# --nodeps: dependency presence is checked by the caller; PKGDEST keeps the artifact here.
PKGDEST="$PWD" SRCDEST="$PWD" BUILDDIR="$PWD/src" makepkg --force --nodeps --nocheck "$@"
ls -la "$PWD"/*.pkg.tar.zst
