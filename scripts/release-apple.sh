#!/bin/sh
# Package universal sc and sc-mcp, plus per-arch tarballs and a dmg.
# Signs and notarizes when APPLE_CERTIFICATE_P12 and the notary secrets are set.
# The per-arch binaries are signed before the tarballs are packed, so every
# shipped binary is signed. Bare binaries cannot be stapled, so they are
# notarized via a zip submitted alongside the dmg.
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

# Resolve signing secrets and the Developer ID identity before building
# any artifact, so a bad cert fails fast instead of mid-packaging.
signing=false
if [ -n "${APPLE_CERTIFICATE_P12:-}" ]; then
  signing=true
  for v in APPLE_CERTIFICATE_PASSWORD APPLE_NOTARY_ISSUER APPLE_NOTARY_KEY_ID APPLE_NOTARY_KEY; do
    eval "val=\${$v:-}"
    if [ -z "$val" ]; then
      echo "error: $v is not set" >&2
      exit 1
    fi
  done
  security create-keychain -p "" "$keychain"
  security set-keychain-settings -lut 21600 "$keychain"
  security unlock-keychain -p "" "$keychain"
  printf '%s' "$APPLE_CERTIFICATE_P12" | tr -d ' \r\n' | base64 --decode > "$work/cert.p12"
  if ! security import "$work/cert.p12" -k "$keychain" -P "$APPLE_CERTIFICATE_PASSWORD" -T /usr/bin/codesign; then
    echo "error: p12 import failed (check APPLE_CERTIFICATE_P12 and APPLE_CERTIFICATE_PASSWORD)" >&2
    exit 1
  fi
  security set-key-partition-list -S apple-tool:,apple: -s -k "" "$keychain" >/dev/null
  security list-keychains -d user -s "$keychain"
  identity=$(security find-identity -v -p codesigning "$keychain" | awk -F'"' '/Developer ID Application/{print $2; exit}')
  if [ -z "$identity" ]; then
    echo "error: no 'Developer ID Application' codesigning identity in $keychain (check APPLE_CERTIFICATE_P12)" >&2
    exit 1
  fi
fi

# Sign the per-arch binaries first so the tarballs ship signed code.
# lipo preserves no signature, so the universal binaries are re-signed
# right after they are created.
if [ "$signing" = true ]; then
  codesign --force --options runtime --timestamp --sign "$identity" \
    "$arm/sc" "$arm/sc-mcp" "$intel/sc" "$intel/sc-mcp"
fi
lipo -create -output "$stage/sc" "$arm/sc" "$intel/sc"
lipo -create -output "$stage/sc-mcp" "$arm/sc-mcp" "$intel/sc-mcp"
if [ "$signing" = true ]; then
  codesign --force --options runtime --timestamp --sign "$identity" "$stage/sc" "$stage/sc-mcp"
fi
lipo -info "$stage/sc" | grep -q x86_64
lipo -info "$stage/sc" | grep -q arm64
lipo -info "$stage/sc-mcp" | grep -q x86_64
lipo -info "$stage/sc-mcp" | grep -q arm64
cp "$root/LICENSE" "$root/README.md" "$stage/"

# A prerelease tag (v0.1.0-rc.2) may ship binaries that report the base
# Cargo.toml version (0.1.0); a final tag must match exactly.
base="${ver%%-*}"
got=$("$stage/sc" --version)
if [ "$got" != "sc $ver" ] && [ "$got" != "sc $base" ]; then
  echo "error: sc --version is '$got', tag is $GITHUB_REF_NAME" >&2
  exit 1
fi
got=$("$stage/sc-mcp" --version)
if [ "$got" != "$ver" ] && [ "$got" != "$base" ]; then
  echo "error: sc-mcp --version is '$got', tag is $GITHUB_REF_NAME" >&2
  exit 1
fi

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

# Offline check that every shipped binary carries a valid signature.
if [ "$signing" = true ]; then
  codesign --verify --strict --verbose \
    "$arm/sc" "$arm/sc-mcp" "$intel/sc" "$intel/sc-mcp" \
    "$stage/sc" "$stage/sc-mcp"
fi

dmg="$dist/sc-v${ver}-universal-apple-darwin.dmg"
signed=false
if [ "$signing" = true ]; then
  rm -f "$dmg"
  hdiutil create -volname "Scorecard $ver" -srcfolder "$stage" -ov -format UDZO "$dmg"
  codesign --force --timestamp --sign "$identity" "$dmg"
  # Accept the .p8 either as PEM text or base64 of the PEM file.
  case "$APPLE_NOTARY_KEY" in
    *"BEGIN PRIVATE KEY"*) printf '%s\n' "$APPLE_NOTARY_KEY" > "$work/AuthKey.p8" ;;
    *) printf '%s' "$APPLE_NOTARY_KEY" | tr -d ' \r\n' | base64 --decode > "$work/AuthKey.p8" ;;
  esac
  set +e
  xcrun notarytool submit "$dmg" --wait --output-format json \
    --issuer "$APPLE_NOTARY_ISSUER" \
    --key-id "$APPLE_NOTARY_KEY_ID" \
    --key "$work/AuthKey.p8" > "$work/notary.json"
  rc=$?
  set -e
  cat "$work/notary.json"
  echo
  sub_id=$(plutil -extract id raw -o - "$work/notary.json" 2>/dev/null || true)
  status=$(plutil -extract status raw -o - "$work/notary.json" 2>/dev/null || true)
  if [ "$rc" -ne 0 ] || [ "$status" != "Accepted" ]; then
    echo "error: notarization failed (notarytool exit $rc, status ${status:-unknown})" >&2
    if [ -n "$sub_id" ]; then
      xcrun notarytool log "$sub_id" \
        --issuer "$APPLE_NOTARY_ISSUER" \
        --key-id "$APPLE_NOTARY_KEY_ID" \
        --key "$work/AuthKey.p8" || true
    fi
    exit 1
  fi
  xcrun stapler staple "$dmg"
  xcrun stapler validate "$dmg"
  # Bare binaries cannot be stapled, so notarize them via a zip of the
  # signed per-arch binaries. Gatekeeper picks up the ticket online.
  rm -rf "$work/notary-bin"
  mkdir -p "$work/notary-bin/aarch64-apple-darwin" "$work/notary-bin/x86_64-apple-darwin"
  cp "$arm/sc" "$arm/sc-mcp" "$work/notary-bin/aarch64-apple-darwin/"
  cp "$intel/sc" "$intel/sc-mcp" "$work/notary-bin/x86_64-apple-darwin/"
  (cd "$work" && ditto -c -k --sequesterRsrc --keepParent notary-bin "sc-v${ver}-macos-binaries.zip")
  set +e
  xcrun notarytool submit "$work/sc-v${ver}-macos-binaries.zip" --wait --output-format json \
    --issuer "$APPLE_NOTARY_ISSUER" \
    --key-id "$APPLE_NOTARY_KEY_ID" \
    --key "$work/AuthKey.p8" > "$work/notary-bin.json"
  rc=$?
  set -e
  cat "$work/notary-bin.json"
  echo
  bin_status=$(plutil -extract status raw -o - "$work/notary-bin.json" 2>/dev/null || true)
  if [ "$rc" -ne 0 ] || [ "$bin_status" != "Accepted" ]; then
    echo "error: binary notarization failed (notarytool exit $rc, status ${bin_status:-unknown})" >&2
    exit 1
  fi
  signed=true
else
  rm -f "$dmg"
  hdiutil create -volname "Scorecard $ver" -srcfolder "$stage" -ov -format UDZO "$dmg"
fi

if [ -n "${GITHUB_OUTPUT:-}" ]; then
  echo "signed=$signed" >> "$GITHUB_OUTPUT"
fi
