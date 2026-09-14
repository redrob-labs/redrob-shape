#!/usr/bin/env bash
#
# Applies patches/ to the fetched upstream, in numeric order, failing on the first bad one.
#
# --3way so a patch that has drifted against a bumped upstream reports the conflicting hunk
# rather than silently doing nothing.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

target="build/blender"
[ -d "$target/.git" ] || { echo "error: $target is not a checkout; run fetch-upstream.sh first" >&2; exit 1; }

count=0
for patch in patches/*.patch; do
  [ -e "$patch" ] || continue
  printf '  %-52s ' "$(basename "$patch")"
  if git -C "$target" apply --3way --whitespace=nowarn "$root/$patch" 2>/tmp/patch-err.$$; then
    echo "ok"
    count=$((count + 1))
  else
    echo "FAILED"
    sed 's/^/    /' /tmp/patch-err.$$ >&2
    rm -f /tmp/patch-err.$$
    exit 1
  fi
  rm -f /tmp/patch-err.$$
done

echo "applied $count patch(es)"
