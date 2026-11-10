#!/bin/sh
# Install sc and sc-mcp from a published release into $HOME/.local/bin.
#
#   curl --proto '=https' --tlsv1.2 -fsSL https://raw.githubusercontent.com/moonbase2090/Scorecard/main/install.sh | sh
#
# Set SC_VERSION (for example 0.1.14) to pin a release. Otherwise the script
# installs the latest GitHub release. Linux and macOS use the .tar.gz assets.
# Windows x86_64 uses the .zip when this runs under Git Bash or another Unix shell.
set -eu

repo=moonbase2090/Scorecard
releases=https://github.com/${repo}/releases
source_hint=https://github.com/${repo}#install

die() {
  printf '%s\n' "$*" >&2
  exit 1
}

if ! command -v curl >/dev/null 2>&1; then
  die "error: curl is not installed, so the release asset cannot be downloaded. Install curl and rerun."
fi

os=$(uname -s)
arch=$(uname -m)
ext=tar.gz
case "$os" in
  Darwin)
    case "$arch" in
      arm64 | aarch64) target=aarch64-apple-darwin ;;
      x86_64) target=x86_64-apple-darwin ;;
      *)
        die "error: no sc release for Darwin ${arch}. Install from source: ${source_hint}"
        ;;
    esac
    ;;
  Linux)
    case "$arch" in
      x86_64 | amd64) target=x86_64-unknown-linux-gnu ;;
      aarch64 | arm64) target=aarch64-unknown-linux-gnu ;;
      *)
        die "error: no sc release for Linux ${arch}. Install from source: ${source_hint}"
        ;;
    esac
    ;;
  MINGW* | MSYS* | CYGWIN*)
    ext=zip
    case "$arch" in
      x86_64 | amd64) target=x86_64-pc-windows-msvc ;;
      *)
        die "error: no sc release for Windows ${arch}. Install from source: ${source_hint}"
        ;;
    esac
    ;;
  *)
    die "error: no sc release for ${os} ${arch}. Install from source: ${source_hint}"
    ;;
esac

if [ -z "${HOME:-}" ]; then
  die "error: HOME is not set, so there is no user bin directory. Set HOME and rerun."
fi
dest="$HOME/.local/bin"

version=${SC_VERSION:-}
version=${version#v}
if [ -n "$version" ]; then
  case "$version" in
    "" | .* | *..* | */*)
      die "error: SC_VERSION is not a release version. Use a version such as 0.1.14 and rerun."
      ;;
  esac
  case "$version" in
    *[!0-9A-Za-z._-]*)
      die "error: SC_VERSION is not a release version. Use a version such as 0.1.14 and rerun."
      ;;
  esac
fi

stage=$(mktemp -d "${TMPDIR:-/tmp}/sc-install.XXXXXX")
trap 'rm -rf "$stage"' EXIT

if [ -z "$version" ]; then
  latest="https://api.github.com/repos/${repo}/releases/latest"
  if ! curl --proto '=https' --tlsv1.2 -fsSL -o "$stage/release.json" "$latest"; then
    die "error: could not read the latest sc release from ${latest}. Set SC_VERSION to a published version and rerun."
  fi
  version=$(sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"v\([^"]*\)".*/\1/p' "$stage/release.json" | head -n 1)
  if [ -z "$version" ]; then
    die "error: the latest sc release did not include a v* tag. Set SC_VERSION to a published version and rerun."
  fi
  case "$version" in
    *[!0-9A-Za-z._-]* | *..* | */* | .*)
      die "error: the latest sc release tag is not a version this script can install. Set SC_VERSION to a published version and rerun."
      ;;
  esac
fi

asset="sc-v${version}-${target}.${ext}"
base="${releases}/download/v${version}"
archive="$stage/$asset"

if ! curl --proto '=https' --tlsv1.2 -fsSL -o "$archive" "$base/$asset"; then
  die "error: could not download ${asset} from ${base}. Check SC_VERSION and that this platform is published, then rerun."
fi
if ! curl --proto '=https' --tlsv1.2 -fsSL -o "$stage/SHA256SUMS" "$base/SHA256SUMS"; then
  die "error: could not download SHA256SUMS for v${version} from ${base}. Refusing to install without a checksum."
fi

want=$(awk -v f="$asset" '$2 == f { print $1; exit }' "$stage/SHA256SUMS" | tr -d '\r')
if [ -z "$want" ]; then
  die "error: SHA256SUMS for v${version} has no entry for ${asset}. Refusing to install."
fi
if ! printf '%s\n' "$want" | grep -E '^[0-9a-fA-F]{64}$' >/dev/null; then
  die "error: SHA256SUMS entry for ${asset} is not a SHA-256 digest. Refusing to install."
fi

if command -v sha256sum >/dev/null 2>&1; then
  got=$(sha256sum "$archive" | awk 'NR==1 { print $1 }')
elif command -v shasum >/dev/null 2>&1; then
  got=$(shasum -a 256 "$archive" | awk 'NR==1 { print $1 }')
else
  die "error: sha256sum and shasum are missing, so ${asset} cannot be verified. Install coreutils (sha256sum) or perl (shasum) and rerun."
fi
want=$(printf '%s' "$want" | tr 'ABCDEF' 'abcdef')
got=$(printf '%s' "$got" | tr 'ABCDEF' 'abcdef')
if ! printf '%s\n' "$got" | grep -E '^[0-9a-fA-F]{64}$' >/dev/null; then
  die "error: could not compute a SHA-256 digest for ${asset}. Install sha256sum or shasum and rerun."
fi
if [ "$want" != "$got" ]; then
  die "error: checksum mismatch for ${asset}. Expected ${want}, got ${got}. Refusing to install."
fi

unpack="$stage/unpack"
mkdir -p "$unpack"
if [ "$ext" = zip ]; then
  if command -v python3 >/dev/null 2>&1; then
    python3 - "$archive" "$unpack" <<'PY'
import sys
import zipfile

archive, dest = sys.argv[1], sys.argv[2]
with zipfile.ZipFile(archive) as zf:
    for info in zf.infolist():
        parts = info.filename.replace("\\", "/").split("/")
        if info.filename.startswith(("/", "\\")) or ".." in parts:
            sys.stderr.write(
                "error: %s contains an unsafe path. Refusing to install.\n" % archive
            )
            sys.exit(1)
    zf.extractall(dest)
PY
  elif command -v unzip >/dev/null 2>&1; then
    if ! names=$(unzip -Z1 "$archive" 2>/dev/null); then
      die "error: unzip could not list ${asset}. Install python3 and rerun."
    fi
    if printf '%s\n' "$names" | grep -E '(^/|(^|/)\.\.(/|$)|\\)' >/dev/null; then
      die "error: ${asset} contains an unsafe path. Refusing to install."
    fi
    unzip -q "$archive" -d "$unpack"
  else
    die "error: python3 and unzip are missing, so ${asset} cannot be extracted. Install python3 or unzip and rerun."
  fi
else
  if ! names=$(tar -tzf "$archive"); then
    die "error: could not list ${asset}. The download is not a gzip tar archive."
  fi
  if printf '%s\n' "$names" | grep -E '(^/|(^|/)\.\.(/|$))' >/dev/null; then
    die "error: ${asset} contains an unsafe path. Refusing to install."
  fi
  tar -xzf "$archive" -C "$unpack"
fi

root=""
if [ -f "$unpack/sc" ] || [ -f "$unpack/sc.exe" ]; then
  root=$unpack
else
  for dir in "$unpack"/sc-v*; do
    if [ -d "$dir" ]; then
      root=$dir
      break
    fi
  done
fi
if [ -z "$root" ]; then
  die "error: ${asset} has no sc binary. Expected sc inside sc-v<version>-<target>/."
fi
if [ -f "$root/sc.exe" ]; then
  bin=sc.exe
  mcp=sc-mcp.exe
else
  bin=sc
  mcp=sc-mcp
fi
if [ ! -f "$root/$bin" ]; then
  die "error: ${asset} has no sc binary. Expected sc inside sc-v<version>-<target>/."
fi

if ! mkdir -p "$dest"; then
  die "error: could not create ${dest}. Set HOME to a writable directory and rerun."
fi
if ! install -m 755 "$root/$bin" "$dest/$bin"; then
  die "error: could not install ${bin} to ${dest}. Make that directory writable and rerun."
fi
printf 'installed %s v%s (%s) to %s\n' "$bin" "$version" "$target" "$dest"
if [ -f "$root/$mcp" ]; then
  if ! install -m 755 "$root/$mcp" "$dest/$mcp"; then
    die "error: could not install ${mcp} to ${dest}. Make that directory writable and rerun."
  fi
  printf 'installed %s to %s\n' "$mcp" "$dest"
else
  printf 'sc-mcp was not in %s\n' "$asset"
fi

case ":${PATH:-}:" in
  *":${dest}:"*) ;;
  *)
    printf 'Add the user bin directory to PATH:\n  export PATH="%s:$PATH"\n' "$dest"
    ;;
esac
