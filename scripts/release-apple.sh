#!/bin/sh
# Package universal sc and sc-mcp, plus per-arch tarballs, a signed
# Install Scorecard.pkg, and a dmg holding the pkg alongside INSTALL.txt
# and the current README and LICENSE. The dmg window is styled with
# pinned dmgbuild and committed settings art.
# Signs and notarizes when APPLE_CERTIFICATE_P12 and the notary secrets are set.
# The installer .pkg needs a Developer ID Installer identity: it is read
# from the same keychain, or from an optional APPLE_INSTALLER_P12 plus
# APPLE_INSTALLER_PASSWORD. A signed build without one fails loudly;
# an unsigned local build only warns (dry-run, never shipped).
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
  rm -f "$work/cert.p12" "$work/installer.p12" "$work/AuthKey.p8"
  security delete-keychain "$keychain" 2>/dev/null || true
  rm -rf "$work"
}
trap cleanup EXIT INT TERM

mkdir -p "$stage"

# Resolve signing secrets and the Developer ID identity before building
# any artifact, so a bad cert fails fast instead of mid-packaging.
signing=false
installer_identity=""
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
  # The Developer ID Installer certificate chains to Apple's G2
  # intermediate, which a fresh keychain lacks: without it the
  # installer identity search finds nothing and productsign fails.
  # The .cer is public (not a secret) and committed under
  # packaging/certs.
  security import "$root/packaging/certs/DeveloperIDG2CA.cer" -k "$keychain" \
    -T /usr/bin/productsign -T /usr/bin/pkgbuild -T /usr/bin/productbuild
  printf '%s' "$APPLE_CERTIFICATE_P12" | tr -d ' \r\n' | base64 --decode > "$work/cert.p12"
  # This bundle can contain both Developer ID identities. Grant every
  # signing tool access to the Installer private key without a GUI prompt.
  if ! security import "$work/cert.p12" -k "$keychain" -P "$APPLE_CERTIFICATE_PASSWORD" \
    -T /usr/bin/codesign -T /usr/bin/productbuild -T /usr/bin/productsign -T /usr/bin/pkgbuild; then
    echo "error: p12 import failed (check APPLE_CERTIFICATE_P12 and APPLE_CERTIFICATE_PASSWORD)" >&2
    exit 1
  fi
  security set-key-partition-list -S apple-tool:,apple: -s -k "" "$keychain"
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
lipo -info "$stage/sc-mcp" | grep -q x86_64
# verify_arch order is x86_64 then arm64 (reversed fails on newer
# lipo); older lipo takes one arch per flag, so call it twice.
for bin in "$stage/sc" "$stage/sc-mcp"; do
  lipo "$bin" -verify_arch x86_64
  lipo "$bin" -verify_arch arm64
done
cp "$root/LICENSE" "$root/README.md" "$stage/"
# LICENSE ships in the image as LICENSE.txt so Finder shows a text
# icon instead of the generic '?' document.
cp "$stage/LICENSE" "$stage/LICENSE.txt"
rm "$stage/LICENSE"
cp "$root/packaging/INSTALL.txt" "$stage/"

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

# Resolve the Developer ID Installer identity for the .pkg. The main
# APPLE_CERTIFICATE_P12 usually holds only the Application identity,
# so an optional APPLE_INSTALLER_P12 plus APPLE_INSTALLER_PASSWORD is
# accepted as a second source.
installer_identity=""
if [ "$signing" = true ]; then
  installer_identity=$(security find-identity -v -p basic "$keychain" | awk -F'"' '/Developer ID Installer/{print $2; exit}')
  if [ -z "$installer_identity" ] && [ -n "${APPLE_INSTALLER_P12:-}" ]; then
    if [ -z "${APPLE_INSTALLER_PASSWORD:-}" ]; then
      echo "error: APPLE_INSTALLER_PASSWORD is not set" >&2
      exit 1
    fi
    printf '%s' "$APPLE_INSTALLER_P12" | tr -d ' \r\n' | base64 --decode > "$work/installer.p12"
    if ! security import "$work/installer.p12" -k "$keychain" -P "$APPLE_INSTALLER_PASSWORD" -T /usr/bin/productsign -T /usr/bin/pkgbuild -T /usr/bin/productbuild; then
      echo "error: installer p12 import failed (check APPLE_INSTALLER_P12 and APPLE_INSTALLER_PASSWORD)" >&2
      exit 1
    fi
    installer_identity=$(security find-identity -v -p basic "$keychain" | awk -F'"' '/Developer ID Installer/{print $2; exit}')
  fi
  if [ -z "$installer_identity" ]; then
    echo "error: no 'Developer ID Installer' identity (add it to APPLE_CERTIFICATE_P12 or set APPLE_INSTALLER_P12 + APPLE_INSTALLER_PASSWORD); refusing to ship an unsigned pkg" >&2
    exit 1
  fi
  # set-key-partition-list only stamps keys already in the keychain, so
  # re-apply it after an optional APPLE_INSTALLER_P12 import. Without this,
  # a newly imported key can miss the apple-tool partition and productbuild
  # can block on an invisible approval prompt.
  security set-key-partition-list -S apple-tool:,apple: -s -k "" "$keychain"
fi

# macOS runners have no `timeout(1)`. Bound productbuild with python3
# (already required for dmgbuild) so a future stall fails the step in
# 5 minutes with a traceback instead of hanging to cancel.
run_with_timeout() {
  secs=$1
  shift
  python3 -c 'import subprocess, sys; subprocess.run(sys.argv[2:], check=True, timeout=int(sys.argv[1]))' "$secs" "$@"
}

# Build Install Scorecard.pkg: the universal binaries go to
# /usr/local/bin under the com.moonbase2090.scorecard identifier.
# The product pkg lands in dist/ (release asset + checksums) and a
# copy rides in the DMG next to INSTALL.txt.
mkdir -p "$work/pkgroot/usr/local/bin"
cp "$stage/sc" "$stage/sc-mcp" "$work/pkgroot/usr/local/bin/"
pkgbuild --root "$work/pkgroot" --identifier com.moonbase2090.scorecard \
  --version "$ver" --install-location / \
  --scripts "$root/packaging/scripts" "$work/scorecard-component.pkg"
sed "s/@VER@/$ver/g" "$root/packaging/distribution.xml.in" > "$work/distribution.xml"
pkg="$dist/sc-v${ver}-macos.pkg"
if [ -n "$installer_identity" ]; then
  echo "installer identity: $installer_identity"
  security find-identity -v -p basic "$keychain" | head -n 5
  echo "productbuild --sign starting (5-minute guard)"
  run_with_timeout 300 productbuild --distribution "$work/distribution.xml" --package-path "$work" \
    --sign "$installer_identity" "$pkg"
  echo "productbuild done: $pkg"
else
  echo "warning: building an unsigned pkg (no installer identity; local dry-run only, never ship this)" >&2
  productbuild --distribution "$work/distribution.xml" --package-path "$work" "$pkg"
fi
cp "$pkg" "$stage/Install Scorecard.pkg"

dmg="$dist/sc-v${ver}-universal-apple-darwin.dmg"
build_dmg() {
  # dmgbuild writes the styled Finder window (.DS_Store) directly and
  # works headless, unlike AppleScript-driven tools. Pinned so local
  # and CI builds agree; invoked as a module so no PATH setup is needed.
  # Installed into a private venv: Homebrew Python on macOS runners is
  # externally managed (PEP 668) and refuses pip installs, even --user.
  venv="${RUNNER_TEMP:-${TMPDIR:-/tmp}}/sc-dmgbuild-venv"
  if ! "$venv/bin/python" -c "import dmgbuild" 2>/dev/null; then
    rm -rf "$venv"
    { python3 -m venv "$venv" &&
      "$venv/bin/python" -m pip install --quiet --disable-pip-version-check "dmgbuild==1.6.5"; } || {
      echo "error: cannot install dmgbuild==1.6.5" >&2
      exit 1
    }
  fi
  rm -f "$dmg"
  "$venv/bin/python" -m dmgbuild -D "stage=$stage" -D "packaging=$root/packaging" -s "$root/packaging/dmg-settings.py" "Scorecard $ver" "$dmg"
}
signed=false
if [ "$signing" = true ]; then
  case "$APPLE_NOTARY_KEY" in
    *"BEGIN PRIVATE KEY"*) printf '%s\n' "$APPLE_NOTARY_KEY" > "$work/AuthKey.p8" ;;
    *) printf '%s' "$APPLE_NOTARY_KEY" | tr -d ' \r\n' | base64 --decode > "$work/AuthKey.p8" ;;
  esac
  build_dmg
  codesign --force --timestamp --sign "$identity" "$dmg"
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
  # The installer pkg notarizes and staples on its own, so Gatekeeper
  # accepts it even when opened outside the DMG.
  set +e
  xcrun notarytool submit "$pkg" --wait --output-format json \
    --issuer "$APPLE_NOTARY_ISSUER" \
    --key-id "$APPLE_NOTARY_KEY_ID" \
    --key "$work/AuthKey.p8" > "$work/notary-pkg.json"
  rc=$?
  set -e
  cat "$work/notary-pkg.json"
  echo
  pkg_sub_id=$(plutil -extract id raw -o - "$work/notary-pkg.json" 2>/dev/null || true)
  pkg_status=$(plutil -extract status raw -o - "$work/notary-pkg.json" 2>/dev/null || true)
  if [ "$rc" -ne 0 ] || [ "$pkg_status" != "Accepted" ]; then
    echo "error: pkg notarization failed (notarytool exit $rc, status ${pkg_status:-unknown})" >&2
    if [ -n "$pkg_sub_id" ]; then
      xcrun notarytool log "$pkg_sub_id" \
        --issuer "$APPLE_NOTARY_ISSUER" \
        --key-id "$APPLE_NOTARY_KEY_ID" \
        --key "$work/AuthKey.p8" || true
    fi
    exit 1
  fi
  xcrun stapler staple "$pkg"
  xcrun stapler validate "$pkg"
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
  build_dmg
fi

if [ -n "${GITHUB_OUTPUT:-}" ]; then
  echo "signed=$signed" >> "$GITHUB_OUTPUT"
fi
