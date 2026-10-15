#!/usr/bin/env bash
# macOS universal release artifact:
#   dist/release/redrob-shape-<version>-macos-universal.dmg   "Redrob Shape.app" on a drag-to-Applications DMG
#
#   packaging/macos/bundle.sh <version>
#
# Signing (env):
#   MACOS_SIGN_IDENTITY   codesign identity (name or SHA-1). Default "-" = ad-hoc (local testing;
#                         Gatekeeper rejects the result on other Macs). In CI, import-cert.sh sets it.
#   MACOS_KEYCHAIN        keychain holding the identity (optional)
# Notarization (env; needs a real identity and one complete set):
#   APPLE_NOTARY_API_KEY_P8_BASE64, APPLE_NOTARY_API_KEY_ID, APPLE_NOTARY_API_ISSUER_ID (API key), or
#   APPLE_ID, APPLE_PASSWORD (app-specific password), APPLE_TEAM_ID
# Without a real identity the bundle is ad-hoc signed and not notarized, with a warning.
set -euo pipefail
version="${1:?version}"
root="$(cd "$(dirname "$0")/../.." && pwd)"
here="$root/packaging/macos"
x64="$root/target/x86_64-apple-darwin/release/redrob-shape"
arm="$root/target/aarch64-apple-darwin/release/redrob-shape"
[ -x "$x64" ] && [ -x "$arm" ] || { echo "build both targets first" >&2; exit 1; }

IDENTITY="${MACOS_SIGN_IDENTITY:--}"
name="redrob-shape-$version-macos-universal"
stage="$root/dist/stage/$name"
app="$stage/Redrob Shape.app"
dmg="$root/dist/release/$name.dmg"
rm -rf "$stage" && mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources" "$root/dist/release"

# ---- notarization credentials ------------------------------------------------------------------
NOTARIZE=0
NOTARY_AUTH=()
if [ "$IDENTITY" = "-" ]; then
  echo "::warning::macOS: ad-hoc signing (no MACOS_SIGN_IDENTITY); the DMG is not notarized"
elif [ -n "${APPLE_NOTARY_API_KEY_P8_BASE64:-}" ] && [ -n "${APPLE_NOTARY_API_KEY_ID:-}" ] &&
  [ -n "${APPLE_NOTARY_API_ISSUER_ID:-}" ]; then
  # App Store Connect API key (the redrob-labs organization secrets). Written 0600 to the
  # runner's temp dir and removed on exit.
  NOTARY_KEY="${RUNNER_TEMP:-${TMPDIR:-/tmp}}/AuthKey_$APPLE_NOTARY_API_KEY_ID.p8"
  (umask 077 && printf '%s' "$APPLE_NOTARY_API_KEY_P8_BASE64" | base64 --decode >"$NOTARY_KEY")
  trap 'rm -f "$NOTARY_KEY"' EXIT
  NOTARY_AUTH=(--key "$NOTARY_KEY" --key-id "$APPLE_NOTARY_API_KEY_ID" --issuer "$APPLE_NOTARY_API_ISSUER_ID")
  NOTARIZE=1
elif [ -n "${APPLE_ID:-}" ] && [ -n "${APPLE_PASSWORD:-}" ] && [ -n "${APPLE_TEAM_ID:-}" ]; then
  NOTARY_AUTH=(--apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" --team-id "$APPLE_TEAM_ID")
  NOTARIZE=1
else
  echo "::warning::macOS: no notarization credentials (APPLE_NOTARY_API_KEY_* or APPLE_ID/APPLE_PASSWORD/APPLE_TEAM_ID); signed but not notarized"
fi
echo "==> Redrob Shape $version for macOS (universal), identity: $IDENTITY, notarize: $NOTARIZE"

sign() {
  # Hardened runtime + secure timestamp for a real identity; ad-hoc can't be timestamped.
  local ts=(--timestamp)
  [ "$IDENTITY" = "-" ] && ts=(--timestamp=none)
  local kc=()
  [ -n "${MACOS_KEYCHAIN:-}" ] && kc=(--keychain "$MACOS_KEYCHAIN")
  codesign --force --sign "$IDENTITY" ${kc[@]+"${kc[@]}"} "${ts[@]}" "$@"
}

notarize() {
  local file="$1" out id status
  echo "==> notarizing $(basename "$file") (this can take a few minutes)"
  out="$(xcrun notarytool submit "$file" "${NOTARY_AUTH[@]}" --wait --timeout 1h --output-format json)" || true
  echo "$out"
  id="$(printf '%s' "$out" | plutil -extract id raw -o - - 2>/dev/null || true)"
  status="$(printf '%s' "$out" | plutil -extract status raw -o - - 2>/dev/null || true)"
  if [ "$status" != "Accepted" ]; then
    if [ -n "$id" ]; then xcrun notarytool log "$id" "${NOTARY_AUTH[@]}" || true; fi
    echo "error: notarization of $(basename "$file") failed (status: ${status:-unknown})" >&2
    exit 1
  fi
}

# ---- Redrob Shape.app --------------------------------------------------------------------------
lipo -create "$x64" "$arm" -output "$app/Contents/MacOS/Redrob Shape"
chmod +x "$app/Contents/MacOS/Redrob Shape"
lipo -info "$app/Contents/MacOS/Redrob Shape"

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
  "$here/Info.plist.in" >"$app/Contents/Info.plist"
plutil -lint "$app/Contents/Info.plist"
printf 'APPL????' >"$app/Contents/PkgInfo"

# Sign inside-out: the executable, then the bundle (no --deep on the final signature).
sign --options runtime --entitlements "$here/entitlements.plist" "$app/Contents/MacOS/Redrob Shape"
sign --options runtime --entitlements "$here/entitlements.plist" "$app"
codesign --verify --strict --deep --verbose=2 "$app"

if [ "$NOTARIZE" = 1 ]; then
  ditto -c -k --keepParent "$app" "$stage/notarize.zip"
  notarize "$stage/notarize.zip"
  rm -f "$stage/notarize.zip"
  xcrun stapler staple "$app"
  xcrun stapler validate "$app"
  spctl --assess --type execute -vvv "$app"
fi

# ---- DMG ---------------------------------------------------------------------------------------
echo "==> building $dmg"
dmgstage="$root/dist/stage/dmg"
rm -rf "$dmgstage" && mkdir -p "$dmgstage"
ditto "$app" "$dmgstage/Redrob Shape.app"
ln -s /Applications "$dmgstage/Applications"
cp "$root/README.md" "$root/LICENSE-MIT" "$root/LICENSE-APACHE" "$root/NOTICE" "$dmgstage/"
rm -f "$dmg" "$root/dist/stage/raw.dmg"
# makehybrid + convert builds the image without attaching a device, unlike `create -srcfolder`,
# which is flaky on CI runners ("Resource busy").
hdiutil makehybrid -hfs -hfs-volume-name "Redrob Shape $version" -hfs-openfolder "$dmgstage" -o "$root/dist/stage/raw.dmg" "$dmgstage"
hdiutil convert "$root/dist/stage/raw.dmg" -format UDZO -imagekey zlib-level=9 -o "$dmg"
rm -f "$root/dist/stage/raw.dmg"
sign "$dmg"
codesign --verify --strict --verbose=2 "$dmg"
if [ "$NOTARIZE" = 1 ]; then
  notarize "$dmg"
  xcrun stapler staple "$dmg"
  xcrun stapler validate "$dmg"
  spctl --assess --type open --context context:primary-signature -vvv "$dmg"
fi
echo "wrote dist/release/$name.dmg"
ls -lh "$dmg"
