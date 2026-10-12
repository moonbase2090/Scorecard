#!/usr/bin/env bash
# Install sc and sc-mcp from a release .tar.gz into $HOME/.local/bin.
# Uses a new directory under RUNNER_TEMP (or /tmp) per install so a reused
# staging path cannot leave a stale flat-layout sc ahead of a nested bundle.
set -euo pipefail
export COPYFILE_DISABLE=1

if [ "$#" -ne 1 ] || [ ! -f "$1" ]; then
  echo "usage: install-release-tarball.sh PATH_TO_TAR_GZ" >&2
  exit 2
fi
archive="$1"
base="${RUNNER_TEMP:-${TMPDIR:-/tmp}}"
stage="$(mktemp -d "${base%/}/sc-install.XXXXXX")"
trap 'rm -rf "$stage"' EXIT

cp "$archive" "$stage/scorecard.tar.gz"
tar -xzf "$stage/scorecard.tar.gz" -C "$stage"
root="$stage"
if [ -f "$stage/sc" ]; then
  : # flat layout (releases before nested packaging, e.g. v0.1.6)
elif comp="$(find "$stage" -maxdepth 1 -type d -name 'sc-v*' | head -1)" && [ -n "$comp" ]; then
  root="$comp"
else
  echo "release archive has no sc binary" >&2
  exit 1
fi
mkdir -p "$HOME/.local/bin"
install -m 755 "$root/sc" "$HOME/.local/bin/sc"
if [ -f "$root/sc-mcp" ]; then
  install -m 755 "$root/sc-mcp" "$HOME/.local/bin/sc-mcp"
fi
