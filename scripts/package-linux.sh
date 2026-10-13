#!/usr/bin/env bash
#
# Packs an installed Linux build into the tarball the release carries:
#
#   redrob-shape-<version>-linux-x86_64.tar.gz
#
# Usage: scripts/package-linux.sh <install-dir> [out-dir]
#   <install-dir>  the CMAKE_INSTALL_PREFIX of a Linux build: the directory holding `blender`
#   [out-dir]      where the tarball goes (default: dist/)
#
# The version is REDROB_VERSION, because the name is checked against it (check-asset-names.py) and
# the release tag has to equal it too (sign-and-publish.yml, probe).
#
# Layout inside, the one Redrob Launcher unpacks (redrob-launcher, crates/native/src/install.rs,
# tree::unpack): one top folder with bin/redrob-shape in it, which the launcher keeps whole under
# its apps/ folder and runs. Blender finds its 5.3/ datafiles next to the REAL executable, so the
# tree moves intact under lib/redrob-shape/ and bin/redrob-shape is a shell script that execs it.
# A person without the launcher runs the same script after tar -xzf.
#
#   redrob-shape-<version>-linux-x86_64/
#     bin/redrob-shape               exec ../lib/redrob-shape/blender "$@"
#     lib/redrob-shape/              blender, blender-launcher, redrob-shape-agent, 5.3/, lib/, ...
#
# Build-time tools that the install prefix also receives (datatoc, makesdna, makesrna, msgfmt,
# shader_tool, zstd_compress) are not part of the app and are left out, as the first release did.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
install_dir="${1:?usage: $0 <install-dir> [out-dir]}"
out_dir="${2:-$root/dist}"
version="$(tr -d '[:space:]' < "$root/REDROB_VERSION")"

[ -x "$install_dir/blender" ] || { echo "error: $install_dir has no blender executable" >&2; exit 1; }
[ -x "$install_dir/redrob-shape-agent" ] || { echo "error: $install_dir has no redrob-shape-agent" >&2; exit 1; }
[ -d "$install_dir/5.3" ] || { echo "error: $install_dir has no 5.3/ datafiles" >&2; exit 1; }

name="redrob-shape-$version-linux-x86_64"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
top="$work/$name"
mkdir -p "$top/bin" "$top/lib/redrob-shape"

# The app tree, minus the build tools. --no-times is not wanted here: a tarball keeps mtimes.
rsync -a \
  --exclude /datatoc --exclude /makesdna --exclude /makesrna --exclude /msgfmt \
  --exclude /shader_tool --exclude /zstd_compress \
  "$install_dir/" "$top/lib/redrob-shape/"

cat > "$top/bin/redrob-shape" <<'EOF'
#!/bin/sh
# Redrob Shape. The app lives in ../lib/redrob-shape, next to its own datafiles.
here="$(cd "$(dirname "$(readlink -f "$0")")" && pwd)"
exec "$here/../lib/redrob-shape/blender" "$@"
EOF
chmod 755 "$top/bin/redrob-shape"

mkdir -p "$out_dir"
out="$out_dir/$name.tar.gz"
# --owner/--group 0: the tarball is installed on another machine; this build host's uid means
# nothing there. gzip -n: no timestamp in the gzip header.
tar -C "$work" --owner=0 --group=0 --numeric-owner -cf - "$name" | gzip -n -6 > "$out"

python3 "$root/scripts/check-asset-names.py" --version "$version" "$out"
printf '%s  %s bytes\n' "$out" "$(stat -c%s "$out")"
