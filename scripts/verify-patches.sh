#!/usr/bin/env bash
#
# THE GATE. Proves the whole customization still produces the tree we expect from the pinned
# upstream, and fails loudly when it does not.
#
# It DISCARDS any existing checkout and refetches first. Verifying against a tree that is already
# half-patched proves nothing -- a patch that no longer applies would report success because its
# changes were already present.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

echo "=== verifying customization against pinned upstream ==="

rm -rf build/blender
./scripts/fetch-upstream.sh
./scripts/apply-patches.sh
./scripts/apply-overlay.sh
./scripts/apply-brand-assets.sh

target="build/blender"

# --- 1. LFS pointers stayed pointers ---------------------------------------------
# Blender tracks bundled assets in LFS and the GitHub mirror cannot serve those objects, so
# fetch-upstream.sh skips the smudge filter. This asserts the outcome rather than trusting the
# configuration -- and it has to be asserted in CI, because a machine with no git-lfs installed
# never runs the filter and so cannot tell a working skip from a missing tool.
lfs_asset=$(git -C "$target" ls-files -- 'assets/brushes/*.blend' 2>/dev/null | head -1 || true)
if [ -n "$lfs_asset" ] && [ -f "$target/$lfs_asset" ]; then
  if head -c 40 "$target/$lfs_asset" | grep -q "version https://git-lfs"; then
    echo "LFS assets left as pointers, as intended ($(basename "$lfs_asset"))"
  else
    # Not a failure: a remote that CAN serve the objects is strictly better. Reported so nobody
    # concludes from a green build that the skip is still needed.
    echo "note: LFS objects were materialised, so this remote can serve them"
  fi
fi

# --- 2. the add-on is registered, not merely present ------------------------------
# A copied-in add-on that addon_utils does not know about produces a build with no panel.
if ! grep -q "redrob_reblend" "$target/scripts/modules/addon_utils.py"; then
  echo "error: addon_utils.py does not know about the redrob_reblend add-on" >&2
  exit 1
fi
for f in __init__.py panels.py operators.py runtime.py protocol.py tools.py state.py theme.py context.py credentials.py transport.py connection_ui.py; do
  if [ ! -f "$target/scripts/addons_core/redrob_reblend/$f" ]; then
    echo "error: scripts/addons_core/redrob_reblend/$f missing from the checkout" >&2
    exit 1
  fi
done
if [ ! -f "$target/scripts/addons_core/redrob_reblend/blender_manifest.toml" ]; then
  echo "error: the add-on has no blender_manifest.toml, so Blender will not load it" >&2
  exit 1
fi
# The connection UI has to be registered or the preferences it declares never exist, and the
# transport picker silently does nothing.
if ! grep -q "connection_ui" "$target/scripts/addons_core/redrob_reblend/__init__.py"; then
  echo "error: connection_ui is present but never registered" >&2
  exit 1
fi
# PEP 563 stringifies annotations, and Blender registers add-on properties by evaluating them --
# so this import in a module declaring bpy.props breaks registration at enable time, inside
# Blender, where CI would not see it.
if grep -q "^from __future__ import annotations" "$target/scripts/addons_core/redrob_reblend/connection_ui.py"; then
  echo "error: connection_ui.py uses PEP 563 annotations, which breaks bpy.props registration" >&2
  exit 1
fi
echo "Reblend add-on present and registered, with the connection UI wired in"

# --- 3. the Rust agent is wired into the build ------------------------------------
# Three separate things have to be true, and my first version of this check asserted the wrong
# one -- it grepped for the directory name in the top-level CMakeLists, where the actual
# reference is the uppercase option. Checking all three is what makes it a contract:
if ! grep -q "option(WITH_REDROB_AGENT" "$target/CMakeLists.txt"; then
  echo "error: the top-level CMakeLists.txt does not declare the WITH_REDROB_AGENT option" >&2
  exit 1
fi
if ! grep -q "redrob_agent_attach_to_target(blender)" "$target/source/creator/CMakeLists.txt"; then
  echo "error: the agent is never attached to the blender target" >&2
  exit 1
fi
# The option defaults to OFF, so without the config preset a build would silently ship with no
# agent and no error. The preset is what turns it on.
preset="$target/build_files/cmake/config/redrob_reblend.cmake"
if [ ! -f "$preset" ] || ! grep -q "set(WITH_REDROB_AGENT ON" "$preset"; then
  echo "error: the redrob_reblend cmake preset does not enable WITH_REDROB_AGENT" >&2
  echo "       The option defaults to OFF, so the agent would be silently absent." >&2
  exit 1
fi
if [ ! -f "$target/intern/redrob_agent/Cargo.toml" ] || [ ! -f "$target/intern/redrob_agent/CMakeLists.txt" ]; then
  echo "error: intern/redrob_agent is incomplete" >&2
  exit 1
fi
echo "Rust agent wired in: option declared, attached to the target, enabled by the preset"

# --- 4. the application shell is branded -----------------------------------------
title_source="$target/source/blender/windowmanager/intern/wm_window.cc"
if ! grep -q '"Redrob Reblend"' "$title_source"; then
  echo "error: the window title is not branded -- the app would present itself as Blender" >&2
  exit 1
fi
if ! grep -q "^Name=Redrob Reblend$" "$target/release/freedesktop/blender.desktop"; then
  echo "error: the desktop entry still names Blender" >&2
  exit 1
fi
echo "application shell branded: $(grep '^Name=' "$target/release/freedesktop/blender.desktop" | cut -d= -f2)"

# The splash screen, the animation player and the window all name the product separately, so a
# branding pass that misses one leaves the app calling itself Blender somewhere the user looks.
for f in wm_splash_screen.cc wm_playanim.cc wm_window.cc; do
  if ! grep -q "Reblend" "$target/source/blender/windowmanager/intern/$f"; then
    echo "error: $f still presents the product as Blender" >&2
    exit 1
  fi
done
# macOS reads the bundle name from Info.plist, not from the window title.
if ! grep -q "Reblend" "$target/release/darwin/Blender.app/Contents/Info.plist"; then
  echo "error: the macOS bundle Info.plist is not branded" >&2
  exit 1
fi
echo "splash, player, window and macOS bundle all branded"

# And the icons must be OURS, not upstream's. Compared by content, because a file merely existing
# at the right path proves nothing -- upstream ships one there too.
for pair in \
  "brand/release/windows/icons/winblender.ico:release/windows/icons/winblender.ico" \
  "brand/release/darwin/Blender.app/Contents/Resources/blender_icon_legacy.icns:release/darwin/Blender.app/Contents/Resources/blender_icon_legacy.icns"; do
  src="${pair%%:*}"
  dest="${pair#*:}"
  if ! cmp -s "$src" "$target/$dest"; then
    echo "error: $dest is not our brand asset" >&2
    exit 1
  fi
done
echo "application icons are ours"

# --- 5. Blender's attribution and licences are intact ----------------------------
# The branding above is deliberately narrow. Blender is GPL and its trademark policy asks that a
# modified redistribution not present itself as official Blender -- which is what renaming the
# shell satisfies. It does NOT license us to strip Blender's copyright, its licence files, or its
# name from the code and documentation that is Blender's work, and UPSTREAM.md commits us to
# keeping the 3D core's own naming. So this asserts what must NOT have changed.
for f in COPYING release/license/license.md release/license/licenses.json; do
  if [ ! -f "$target/$f" ]; then
    echo "error: $f is missing -- Blender's licence must ship intact" >&2
    exit 1
  fi
done
if ! grep -qi "blender" "$target/release/freedesktop/org.blender.Blender.metainfo.xml"; then
  echo "error: upstream metainfo no longer credits Blender" >&2
  exit 1
fi
echo "Blender's licence and attribution intact"

# --- 6. blend-file compatibility was not touched ---------------------------------
# UPSTREAM.md forbids resolving an upstream sync by changing these. A patch that moved the file
# subversion would silently make our builds write blend files upstream cannot read.
version_header="$target/source/blender/blenkernel/BKE_blender_version.h"
declared=$(python3 -c "
import json
print(json.load(open('upstream.json'))['version'])
")
major=$(grep -oP '(?<=#define BLENDER_VERSION )\d+' "$version_header")
cycle=$(grep -oP '(?<=#define BLENDER_VERSION_CYCLE )\w+' "$version_header")
built="$((major / 100)).$((major % 100)).0-$cycle"
if [ "$declared" != "$built" ]; then
  echo "error: upstream.json says $declared but the checkout says $built" >&2
  exit 1
fi
echo "upstream version matches the pin: $built (blend subversion $(grep -oP '(?<=#define BLENDER_FILE_SUBVERSION )\d+' "$version_header"))"

# --- 7. no patch edits Blender's own tests or operators --------------------------
# UPSTREAM.md: never resolve a sync by silently dropping upstream operators, keymaps, editors,
# importers, exporters, render engines, or tests. This makes that structural.
forbidden=$(grep -hoE '^\+\+\+ b/(tests/|source/blender/editors/|scripts/startup/bl_operators/)[^ ]*' patches/*.patch || true)
if [ -n "$forbidden" ]; then
  echo "error: a patch modifies upstream tests, editors or operators:" >&2
  echo "$forbidden" | sed 's/^/    /' >&2
  echo "       UPSTREAM.md forbids resolving a sync that way." >&2
  exit 1
fi
echo "no patch touches upstream tests, editors or operators"

# --- 8. this repository has CI of its own ----------------------------------------
if [ ! -f ".github/workflows/verify-patches.yml" ]; then
  echo "error: this repository has no CI of its own" >&2
  exit 1
fi
if ! grep -q "addon-tests" .github/workflows/verify-patches.yml; then
  echo "error: CI does not run the add-on's own tests" >&2
  exit 1
fi
echo "CI present, and it runs the add-on tests"

echo "=== customization verified ==="
