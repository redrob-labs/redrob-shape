#!/usr/bin/env bash
#
# Fetches the pinned upstream FreeCAD into build/blender.
#
# Line endings are forced to LF in the clone. Without this, a Windows runner with the default
# core.autocrlf gives a CRLF working tree, and a patch generated on Linux then fails to apply
# against it -- a class of failure that cost the sibling redrob-ide repository three consecutive
# red builds before the cause was found.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

# tr -d '\r' so this works under Git Bash on Windows, where python's stdout is text-mode and a
# bare `read` leaves a carriage return on the value -- git then reports the ref as not found.
repo=$(python3 -c "
import json
print(json.load(open('upstream.json'))['repo'])
" | tr -d '\r')
commit=$(python3 -c "
import json
print(json.load(open('upstream.json'))['commit'])
" | tr -d '\r')

target="build/blender"

if [ -d "$target/.git" ]; then
  have=$(git -C "$target" rev-parse HEAD 2>/dev/null || echo "")
  if [ "$have" = "$commit" ]; then
    echo "upstream already at $commit"
    exit 0
  fi
  echo "upstream checkout is at ${have:-unknown}, wanted $commit -- refetching"
  rm -rf "$target"
fi

mkdir -p build
echo "fetching $repo at $commit"
git init --quiet "$target"
git -C "$target" config core.autocrlf false
git -C "$target" config core.eol lf
git -C "$target" remote add origin "$repo"
git -C "$target" fetch --quiet --depth 1 origin "$commit"
git -C "$target" checkout --quiet FETCH_HEAD

actual=$(git -C "$target" rev-parse HEAD)
if [ "$actual" != "$commit" ]; then
  echo "error: expected $commit, checked out $actual" >&2
  exit 1
fi

echo "upstream at $actual ($(git -C "$target" log -1 --format=%s | cut -c1-60))"
