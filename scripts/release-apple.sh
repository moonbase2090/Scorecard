#!/bin/sh
# Package universal sc and sc-mcp, plus per-arch tarballs and a dmg.
# Signs and notarizes when APPLE_CERTIFICATE_P12 and the notary secrets are set.
#
# Layout: deliverables (tarballs, dmg) go to dist/. Scratch (the stage dir,
# the temporary keychain, key material) lives outside dist/ so artifact
# uploads and the release checksums only ever see shippable files.
set -eu

if [ -z "${GITHUB_REF_NAME:-}" ]; then
  echo "error: GITHUB_REF_NAME is not set (expected a tag like v0.1.0)" >&2
  exit 1
fi
ver="${GITHUB_REF_NAME#v}"
root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
dist="$root/dist"
stage="$root/stage/Scorecard"
arm="$root/target/aarch64-apple-darwin/release"
intel="$root/target/x86_64-apple-darwin/release"

work=$(mktemp -d)
keychain="$work/build.keychain"
cleanup() {
  rm -f "$work/cert.p12" "$work/AuthKey.p8"
  security delete-keychain "$keychain" 2>/dev/null || true
  rm -rf "$work"
}
trap cleanup EXIT INT TERM

mkdir -p "$stage"
lipo -create -output "$stage/sc" "$arm/sc" "$intel/sc"
lipo -create -output "$stage/sc-mcp" "$arm/sc-mcp" "$intel/sc-mcp"
lipo -info "$stage/sc" | grep -q x86_64
lipo -info "$stage/sc" | grep -q arm64
lipo -info "$stage/sc-mcp" | grep -q x86_64
lipo -info "$stage/sc-mcp" | grep -q arm64
cp "$root/LICENSE" "$root/README.md" "$stage/"

"$stage/sc" --version | grep -qx "sc $ver"
"$stage/sc-mcp" --version | grep -qx "$ver"

pack_arch() {
  triple=$1
  src="$root/target/$triple/release"
  tmp=$(mktemp -d)
  cp "$src/sc" "$src/sc-mcp" "$root/LICENSE" "$root/README.md" "$tmp/"
  tar -C "$tmp" -czf "$dist/sc-v${ver}-${triple}.tar.gz" sc sc-mcp LICENSE README.md
  rm -rf "$tmp"
}
mkdir -p "$dist"
pack_arch aarch64-apple-darwin
pack_arch x86_64-apple-darwin

dmg="$dist/sc-v${ver}-universal-apple-darwin.dmg"
signed=false
if [ -n "${APPLE_CERTIFICATE_P12:-}" ]; then
  security create-keychain -p "" "$keychain"
  security set-keychain-settings -lut 21600 "$keychain"
  security unlock-keychain -p "" "$keychain"
  printf '%s' "$APPLE_CERTIFICATE_P12" | base64 --decode > "$work/cert.p12"
  security import "$work/cert.p12" -k "$keychain" -P "$APPLE_CERTIFICATE_PASSWORD" -T /usr/bin/codesign
  security set-key-partition-list -S apple-tool:,apple: -s -k "" "$keychain" >/dev/null
  security list-keychains -d user -s "$keychain"
  identity=$(security find-identity -v -p codesigning "$keychain" | awk -F'"' '/Developer ID Application/{print $2; exit}')
  if [ -z "$identity" ]; then
    echo "error: no 'Developer ID Application' codesigning identity in $keychain (check APPLE_CERTIFICATE_P12)" >&2
    exit 1
  fi
  codesign --force --options runtime --timestamp --sign "$identity" "$stage/sc" "$stage/sc-mcp"
  rm -f "$dmg"
  hdiutil create -volname "Scorecard $ver" -srcfolder "$stage" -ov -format UDZO "$dmg"
  codesign --force --timestamp --sign "$identity" "$dmg"
  printf '%s' "$APPLE_NOTARY_KEY" > "$work/AuthKey.p8"
  xcrun notarytool submit "$dmg" --wait \
    --issuer "$APPLE_NOTARY_ISSUER" \
    --key-id "$APPLE_NOTARY_KEY_ID" \
    --key "$work/AuthKey.p8"
  xcrun stapler staple "$dmg"
  signed=true
else
  rm -f "$dmg"
  hdiutil create -volname "Scorecard $ver" -srcfolder "$stage" -ov -format UDZO "$dmg"
fi

if [ -n "${GITHUB_OUTPUT:-}" ]; then
  echo "signed=$signed" >> "$GITHUB_OUTPUT"
fi
