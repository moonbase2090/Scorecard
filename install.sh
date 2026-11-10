#!/bin/sh
# Install sc and sc-mcp from a GitHub release.
#
#   curl --proto '=https' --tlsv1.2 -fsSL https://raw.githubusercontent.com/moonbase2090/Scorecard/main/install.sh | sh
#
# SC_VERSION       release to install, such as 0.1.14 (default: latest)
# SC_INSTALL_DIR   destination directory (default: $HOME/.local/bin)
# SC_RELEASE_BASE  directory URL that contains SHA256SUMS and the archive
# SC_REPO          GitHub owner/name used when SC_RELEASE_BASE is unset
set -eu

die() {
  printf 'sc install: %s\n' "$1" >&2
  shift
  while [ "$#" -gt 0 ]; do
    printf '%s\n' "$1" >&2
    shift
  done
  exit 1
}

fetch() {
  # stdin is /dev/null so a `curl | sh` pipe is not consumed by these downloads.
  curl -fsSL --retry 3 --retry-delay 1 --max-time 180 -o "$2" "$1" < /dev/null
}

repo=${SC_REPO:-moonbase2090/Scorecard}
owner=${repo%%/*}
name=${repo#*/}
if [ "$owner" = "$repo" ] || [ -z "$owner" ] || [ -z "$name" ] || [ "$name" != "${name#*/}" ]; then
  die "SC_REPO must look like owner/name. Unset SC_REPO to use moonbase2090/Scorecard."
fi
case $owner in
  *[!A-Za-z0-9_.-]*) die "SC_REPO must look like owner/name. Unset SC_REPO to use moonbase2090/Scorecard." ;;
esac
case $name in
  *[!A-Za-z0-9_.-]*) die "SC_REPO must look like owner/name. Unset SC_REPO to use moonbase2090/Scorecard." ;;
esac

if [ -n "${SC_INSTALL_DIR:-}" ]; then
  dest=$SC_INSTALL_DIR
else
  if [ -z "${HOME:-}" ]; then
    die "HOME is not set, so ~/.local/bin cannot be chosen. Set SC_INSTALL_DIR to a writable directory and retry."
  fi
  dest=$HOME/.local/bin
fi

os=$(uname -s)
arch=$(uname -m)
case $os:$arch in
  Darwin:arm64 | Darwin:aarch64) triple=aarch64-apple-darwin ;;
  Darwin:x86_64) triple=x86_64-apple-darwin ;;
  Linux:aarch64 | Linux:arm64) triple=aarch64-unknown-linux-gnu ;;
  Linux:x86_64 | Linux:amd64) triple=x86_64-unknown-linux-gnu ;;
  *)
    die "$os $arch is not a published build for this installer. macOS and Linux on arm64 and x86_64 are supported. Windows builds are the x86_64-pc-windows-msvc.zip asset on the GitHub release. Other systems can install from source: https://github.com/${repo}#install"
    ;;
esac

if [ -n "${SC_RELEASE_BASE:-}" ]; then
  base=${SC_RELEASE_BASE%/}
  ver=${SC_VERSION:-}
  ver=${ver#v}
  if [ -z "$ver" ] || [ "$ver" = "latest" ]; then
    die "SC_RELEASE_BASE is set without a version. Set SC_VERSION to a published release and retry."
  fi
else
  if [ -n "${SC_VERSION:-}" ] && [ "$SC_VERSION" != "latest" ]; then
    ver=${SC_VERSION#v}
  else
    latest=$(curl -fsSL -o /dev/null -w '%{url_effective}' "https://github.com/${repo}/releases/latest" < /dev/null) || latest=
    latest=${latest%/}
    prefix=https://github.com/${repo}/releases/tag/v
    case $latest in
      "$prefix"*)
        tag=${latest##*/}
        ver=${tag#v}
        ;;
      *)
        die "could not resolve the latest GitHub release. Set SC_VERSION to a published release and retry: https://github.com/${repo}/releases/latest"
        ;;
    esac
  fi
  base=https://github.com/${repo}/releases/download/v${ver}
fi

case $ver in
  *[!0-9A-Za-z.-]*)
    die "version '$ver' is not a release tag. Set SC_VERSION to a published release and retry: https://github.com/${repo}/releases/latest"
    ;;
esac
case $ver in
  *[0-9]*) ;;
  *)
    die "version '$ver' is not a release tag. Set SC_VERSION to a published release and retry: https://github.com/${repo}/releases/latest"
    ;;
esac

asset=sc-v${ver}-${triple}.tar.gz
stage=$(mktemp -d "${TMPDIR:-/tmp}/sc-install.XXXXXX")
trap 'rm -rf "$stage"' EXIT

if ! fetch "$base/SHA256SUMS" "$stage/SHA256SUMS"; then
  die "could not download $base/SHA256SUMS. Set SC_VERSION to a published release and retry: https://github.com/${repo}/releases/latest"
fi
if ! fetch "$base/$asset" "$stage/$asset"; then
  die "could not download $base/$asset. macOS and Linux on arm64 and x86_64 are supported. Set SC_VERSION to a published release, or install from source: https://github.com/${repo}#install"
fi
if [ ! -s "$stage/$asset" ]; then
  die "downloaded $asset is empty, so it was not installed. Retry the install."
fi

want=$(
  awk -v f="$asset" '
    {
      name = $2
      sub(/\r$/, "", name)
      sub(/^\*/, "", name)
      if (name == f) {
        print $1
        exit
      }
    }
  ' "$stage/SHA256SUMS" | tr -d '\r\n'
)
want=$(printf '%s' "$want" | tr 'A-F' 'a-f')
case $want in
  *[!0-9a-f]* | "")
    die "SHA256SUMS has no entry for $asset, so the download was not installed. Set SC_VERSION to a published release and retry: https://github.com/${repo}/releases/latest"
    ;;
esac
if [ "${#want}" -ne 64 ]; then
  die "SHA256SUMS has no entry for $asset, so the download was not installed. Set SC_VERSION to a published release and retry: https://github.com/${repo}/releases/latest"
fi

if command -v sha256sum >/dev/null 2>&1; then
  got=$(sha256sum "$stage/$asset" | awk '{ print $1 }')
elif command -v shasum >/dev/null 2>&1; then
  got=$(shasum -a 256 "$stage/$asset" | awk '{ print $1 }')
else
  die "neither sha256sum nor shasum is available, so the download was not installed. Install coreutils (sha256sum) or perl (shasum) and retry."
fi
got=$(printf '%s' "$got" | tr 'A-F' 'a-f')
if [ "$want" != "$got" ]; then
  die "checksum mismatch for $asset, so the download was not installed." "expected $want" "got      $got" "Retry the install. If it fails again, install from source: https://github.com/${repo}#install"
fi

tar -xzf "$stage/$asset" -C "$stage"
root=$stage
if [ -f "$stage/sc" ]; then
  root=$stage
else
  comp=
  for dir in "$stage"/sc-v*; do
    if [ -d "$dir" ]; then
      comp=$dir
      break
    fi
  done
  if [ -n "$comp" ]; then
    root=$comp
  else
    die "archive $asset has no sc binary, so nothing was installed. Set SC_VERSION to a published release and retry: https://github.com/${repo}/releases/latest"
  fi
fi
if [ ! -f "$root/sc" ]; then
  die "archive $asset has no sc binary, so nothing was installed. Set SC_VERSION to a published release and retry: https://github.com/${repo}/releases/latest"
fi

if ! mkdir -p "$dest"; then
  die "cannot create $dest. Set SC_INSTALL_DIR to a writable directory and retry."
fi
if [ ! -w "$dest" ]; then
  die "cannot write to $dest. Set SC_INSTALL_DIR to a writable directory and retry."
fi
install -m 755 "$root/sc" "$dest/sc"
if [ -f "$root/sc-mcp" ]; then
  install -m 755 "$root/sc-mcp" "$dest/sc-mcp"
fi

if ! "$dest/sc" --version >/dev/null 2>&1; then
  rm -f "$dest/sc" "$dest/sc-mcp"
  die "$dest/sc did not run, so it was removed. The published Linux builds need glibc. Install from source: https://github.com/${repo}#install"
fi

version_line=$("$dest/sc" --version)
printf 'installed %s (%s) to %s\n' "$version_line" "$triple" "$dest/sc"
if [ -x "$dest/sc-mcp" ]; then
  printf 'installed %s\n' "$dest/sc-mcp"
fi
case ":$PATH:" in
  *":$dest:"*) ;;
  *)
    printf 'sc is not on PATH. Add this line to your shell profile:\n  export PATH="%s:$PATH"\n' "$dest"
    ;;
esac
