#!/usr/bin/env bash
# Linux archive: the binary, a .desktop file, the icon and the licences.
#   packaging/linux/package.sh <target-triple> <version>
set -euo pipefail
target="${1:?target triple}"
version="${2:?version}"
root="$(cd "$(dirname "$0")/../.." && pwd)"
bin="$root/target/$target/release/redrob-shape"
[ -x "$bin" ] || { echo "missing $bin" >&2; exit 1; }
arch="${target%%-*}"
name="redrob-shape-$version-linux-$arch"
stage="$root/dist/stage/$name"
rm -rf "$stage" && mkdir -p "$stage" "$root/dist/release"
cp "$bin" "$stage/redrob-shape"
cp "$root/packaging/linux/ai.redrob.shape.desktop" "$stage/"
cp "$root/assets/brand/icon-512.png" "$stage/ai.redrob.shape.png"
cp "$root/README.md" "$root/LICENSE-MIT" "$root/LICENSE-APACHE" "$root/NOTICE" "$stage/"
tar -C "$root/dist/stage" -czf "$root/dist/release/$name.tar.gz" "$name"
echo "wrote dist/release/$name.tar.gz"
