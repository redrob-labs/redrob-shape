#!/usr/bin/env bash
# macOS universal bundle: lipo the two target builds into "Redrob Shape.app", ad-hoc sign it so
# Gatekeeper lets it launch after the usual right-click > Open, and zip it.
#   packaging/macos/bundle.sh <version>
# Developer ID signing and notarization come later; nothing here needs a secret.
set -euo pipefail
version="${1:?version}"
root="$(cd "$(dirname "$0")/../.." && pwd)"
x64="$root/target/x86_64-apple-darwin/release/redrob-shape"
arm="$root/target/aarch64-apple-darwin/release/redrob-shape"
[ -x "$x64" ] && [ -x "$arm" ] || { echo "build both targets first" >&2; exit 1; }

name="redrob-shape-$version-macos-universal"
stage="$root/dist/stage/$name"
app="$stage/Redrob Shape.app"
rm -rf "$stage" && mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources" "$root/dist/release"

lipo -create "$x64" "$arm" -output "$app/Contents/MacOS/Redrob Shape"
chmod +x "$app/Contents/MacOS/Redrob Shape"

# Icon: scale the brand PNG with sips (ships with macOS), pack with make-icns.py.
icons="$root/dist/stage/icons"
rm -rf "$icons" && mkdir -p "$icons"
args=()
for s in 16 32 64 128 256 512; do
  sips -z "$s" "$s" "$root/assets/brand/icon-512.png" --out "$icons/$s.png" >/dev/null
  args+=("$s=$icons/$s.png")
done
python3 "$root/packaging/make-icns.py" "$app/Contents/Resources/Redrob Shape.icns" "${args[@]}"

short="${version%%-*}"
sed -e "s/@VERSION@/$version/g" -e "s/@SHORT_VERSION@/$short/g" -e "s/@BUILD_SHA@/${GITHUB_SHA:-local}/g" \
  "$root/packaging/macos/Info.plist.in" > "$app/Contents/Info.plist"
printf 'APPL????' > "$app/Contents/PkgInfo"

codesign --force --deep --sign - "$app"
codesign --verify --deep --strict "$app"
cp "$root/README.md" "$root/LICENSE-MIT" "$root/LICENSE-APACHE" "$root/NOTICE" "$stage/"
(cd "$root/dist/stage" && ditto -c -k --keepParent "$name" "$root/dist/release/$name.zip")
echo "wrote dist/release/$name.zip"
