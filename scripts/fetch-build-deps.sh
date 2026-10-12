#!/usr/bin/env bash
#
# Makes the gate's tree (build/blender, from verify-patches.sh) buildable on this machine:
#
#   1. LFS content. fetch-upstream.sh leaves LFS pointers as pointers because the GitHub mirror
#      cannot serve the objects. Blender's own remote can, so they are pulled from there. Then the
#      brand assets are copied again, because some of them (splash.png) are LFS-tracked upstream
#      and the pull puts upstream's content back.
#   2. Blender's precompiled libraries for one platform, at exactly the commit upstream pins in
#      its lib/<platform> submodule entry, from projects.blender.org.
#
# Usage: scripts/fetch-build-deps.sh <macos_arm64|windows_x64|linux_x64>
#
# The same steps as the local build script for Linux, written once so CI and a person run the
# same thing.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

platform="${1:?usage: $0 <macos_arm64|windows_x64|linux_x64>}"
case "$platform" in
  macos_arm64|windows_x64|linux_x64) ;;
  *) echo "error: unknown platform '$platform'" >&2; exit 2 ;;
esac

src="build/blender"
[ -d "$src/.git" ] || { echo "error: $src missing; run scripts/verify-patches.sh first" >&2; exit 1; }

echo "=== LFS content from projects.blender.org ==="
# fetch-upstream.sh configured the filter to skip; undo that for this checkout only.
git -C "$src" config --unset filter.lfs.smudge || true
git -C "$src" config --unset filter.lfs.process || true
git -C "$src" config --unset filter.lfs.required || true
git -C "$src" lfs install --local --force >/dev/null
git -C "$src" config lfs.url https://projects.blender.org/blender/blender.git/info/lfs
git -C "$src" lfs pull
asset=$(git -C "$src" ls-files -- 'assets/brushes/*.blend' | head -1)
if [ -n "$asset" ] && head -c 40 "$src/$asset" | grep -q "version https://git-lfs"; then
  echo "error: $asset is still an LFS pointer after the pull" >&2
  exit 1
fi

echo "=== brand assets again, over what the LFS pull restored ==="
./scripts/apply-brand-assets.sh

echo "=== precompiled libraries: lib/$platform ==="
sha=$(git -C "$src" ls-tree HEAD "lib/$platform" | awk '{print $3}')
[ -n "$sha" ] || { echo "error: upstream pins no lib/$platform" >&2; exit 1; }
lib="$src/lib/$platform"
if [ "$(git -C "$lib" rev-parse HEAD 2>/dev/null || true)" != "$sha" ]; then
  rm -rf "$lib"
  git init -q "$lib"
  git -C "$lib" config core.autocrlf false
  git -C "$lib" lfs install --local >/dev/null
  git -C "$lib" remote add origin "https://projects.blender.org/blender/lib-$platform.git"
  git -C "$lib" fetch --depth 1 origin "$sha"
  git -C "$lib" checkout -q FETCH_HEAD
  git -C "$lib" lfs pull
fi
echo "lib/$platform at $(git -C "$lib" rev-parse --short HEAD) ($(du -sh "$lib" | cut -f1))"
