#!/usr/bin/env bash
# Put a software OpenGL (Mesa llvmpipe) beside blender.exe, for CI ONLY.
#
# GitHub's Windows runners have no GPU. Their only OpenGL is Windows' own 1.1, and Blender needs
# 4.3, so a window never opens (the log shows only "Unable to initialize a Vulkan 1.2 instance").
# Mesa's llvmpipe gives OpenGL 4.5 on the CPU. Windows loads opengl32.dll from the program's own
# folder before System32, so dropping it there is enough.
#
# Run this AFTER the build is zipped: the shipped build must not carry these files.
#
# Usage: scripts/windows-software-gl.sh <install dir>
set -euo pipefail

install_dir="${1:?usage: windows-software-gl.sh <install dir>}"
[ -f "$install_dir/blender.exe" ] || { echo "no blender.exe in $install_dir" >&2; exit 1; }

# Pinned: pal1000/mesa-dist-win, release-msvc build. Digest from the release's own asset record.
version="26.2.4"
sha256="351fc8c8b695878ffb3eaa044b3ead08672a48b1a045e3c3e3975811df0f6695"
url="https://github.com/pal1000/mesa-dist-win/releases/download/${version}/mesa3d-${version}-release-msvc.7z"

work="${RUNNER_TEMP:-${TMPDIR:-/tmp}}/mesa-${version}"
mkdir -p "$work"
archive="$work/mesa.7z"
curl -fsSL --retry 3 -o "$archive" "$url"
echo "${sha256}  ${archive}" | sha256sum -c -

7z x -y -o"$work/x" "$archive" "x64/*" >/dev/null
# Only the desktop-GL pieces. libglapi.dll exists in older Mesa releases only.
copied=0
for name in opengl32.dll libgallium_wgl.dll libglapi.dll; do
  if [ -f "$work/x/x64/$name" ]; then
    cp "$work/x/x64/$name" "$install_dir/"
    echo "added $name"
    copied=$((copied + 1))
  fi
done
[ -f "$install_dir/opengl32.dll" ] || { echo "Mesa's opengl32.dll is missing from the archive" >&2; exit 1; }
echo "software OpenGL ready: $copied DLLs beside blender.exe (CI only, not in the zip)"
