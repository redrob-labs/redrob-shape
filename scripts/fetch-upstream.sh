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

# Do NOT run the Git LFS smudge filter.
#
# Blender tracks its bundled assets -- assets/brushes/*.blend and similar -- in LFS, and the LFS
# objects are NOT available from the GitHub mirror: the checkout fails with
# "Object does not exist on the server: [404]" and git reports "smudge filter lfs failed", exit 128.
# Blender's canonical remote is projects.blender.org and its LFS storage lives there.
#
# So LFS pointers are left as pointers. That is enough for everything this repository does with the
# tree -- applying patches, copying the overlay, and asserting the result -- because none of our
# patches or overlay files touch an LFS-tracked asset.
#
# A full Blender BUILD does need those assets. That needs `git lfs pull` against a remote that can
# actually serve them, which the GitHub mirror cannot; see README.md.
#
# Worth recording how this was found: it passed locally and failed in CI, because this machine has
# no git-lfs installed and therefore never ran the filter at all. The runner does.
git -C "$target" config filter.lfs.smudge "git-lfs smudge --skip -- %f"
git -C "$target" config filter.lfs.process "git-lfs filter-process --skip"
git -C "$target" config filter.lfs.required false

git -C "$target" remote add origin "$repo"
GIT_LFS_SKIP_SMUDGE=1 git -C "$target" fetch --quiet --depth 1 origin "$commit"
GIT_LFS_SKIP_SMUDGE=1 git -C "$target" checkout --quiet FETCH_HEAD

actual=$(git -C "$target" rev-parse HEAD)
if [ "$actual" != "$commit" ]; then
  echo "error: expected $commit, checked out $actual" >&2
  exit 1
fi

echo "upstream at $actual ($(git -C "$target" log -1 --format=%s | cut -c1-60))"
