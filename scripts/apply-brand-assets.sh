#!/usr/bin/env bash
#
# Copies the brand assets over upstream's icons in the fetched checkout.
#
# WHY THESE ARE COPIED AND NOT PATCHED: they are binary. A binary patch of a 100 KB .ico or a macOS
# .icns conflicts every single time upstream revises its own icon, and the conflict is unreadable --
# a reviewer sees two blobs. Copying is both readable and conflict-free: upstream can change its icon
# freely and ours still lands on top.
#
# It is the same reason the sibling redrob-ide repository copies its sixty brand icons instead of
# carrying them as patches.
#
# Asserts afterwards that every asset arrived and matches, because a silent copy failure produces a
# build that looks branded in its window title and ships Blender's own icon in the dock.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

target="build/blender"
[ -d "$target" ] || { echo "error: $target missing; run fetch-upstream.sh first" >&2; exit 1; }

# source-under-brand/ -> destination-in-checkout
#
# Discovered rather than listed where a directory maps wholesale, so adding a size does not mean
# editing this script.
copy_asset() { # source destination
  local src="$1" dest="$2"
  if [ ! -f "$src" ]; then
    echo "error: brand asset missing from the repository: $src" >&2
    exit 1
  fi
  mkdir -p "$target/$(dirname "$dest")"
  cp "$src" "$target/$dest"
  if ! cmp -s "$src" "$target/$dest"; then
    echo "error: $dest differs from the brand asset after copying" >&2
    exit 1
  fi
  echo "  $dest"
}

copied=0

# Assets whose destination mirrors their path under brand/.
while IFS= read -r file; do
  dest="${file#brand/}"
  copy_asset "$file" "$dest"
  copied=$((copied + 1))
done < <(find brand/release -type f | sort)

# The freedesktop icon sizes, which live under a different root in the checkout.
while IFS= read -r file; do
  dest="release/freedesktop/icons/${file#brand/}"
  copy_asset "$file" "$dest"
  copied=$((copied + 1))
done < <(find brand -mindepth 1 -maxdepth 1 -type d -name '[0-9]*x[0-9]*' -exec find {} -type f \; | sort)

if [ "$copied" -eq 0 ]; then
  echo "error: no brand assets were copied, so the build would ship Blender's own icons" >&2
  exit 1
fi

echo "brand assets applied and verified: $copied file(s)"
