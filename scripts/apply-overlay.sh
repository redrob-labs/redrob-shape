#!/usr/bin/env bash
#
# Copies overlay/ into the fetched upstream.
#
# These are files that exist ONLY in this product -- the OpenCAD AI workbench, the branding, the
# preference pack. A patch that creates a whole new file is a less readable copy of that file and
# conflicts on nothing, so they are stored as files and copied.
#
# Asserts afterwards that every overlay file arrived and matches, because a silent copy failure
# would produce a build that looks like upstream and is branded as ours.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

target="build/blender"
[ -d "$target" ] || { echo "error: $target missing; run fetch-upstream.sh first" >&2; exit 1; }

copied=0
while IFS= read -r file; do
  rel="${file#overlay/}"
  mkdir -p "$target/$(dirname "$rel")"
  cp "$file" "$target/$rel"
  copied=$((copied + 1))
done < <(find overlay -type f | sort)

while IFS= read -r file; do
  rel="${file#overlay/}"
  if [ ! -f "$target/$rel" ]; then
    echo "error: $rel did not arrive in the checkout" >&2
    exit 1
  fi
  if ! cmp -s "$file" "$target/$rel"; then
    echo "error: $rel differs from the overlay after copying" >&2
    exit 1
  fi
done < <(find overlay -type f | sort)

echo "overlay applied and verified: $copied file(s)"
