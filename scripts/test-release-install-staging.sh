#!/usr/bin/env bash
# Regression: nested install must not pick up a stale flat sc left in the parent staging area.
set -euo pipefail
export COPYFILE_DISABLE=1
root="$(cd "$(dirname "$0")/.." && pwd)"
install_sh="$root/scripts/install-release-tarball.sh"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

parent="$work/runner-temp"
mkdir -p "$parent"
# Legacy flat install left sc directly in a reused staging directory.
stale="$parent/sc-install"
mkdir -p "$stale"
printf '#!/bin/sh\necho sc 0.1.5\n' >"$stale/sc"
chmod +x "$stale/sc"

bundle="sc-v0.1.6-x86_64-unknown-linux-gnu"
nested="$work/nested/$bundle"
mkdir -p "$nested"
printf '#!/bin/sh\necho sc 0.1.6\n' >"$nested/sc"
printf '#!/bin/sh\necho mcp\n' >"$nested/sc-mcp"
chmod +x "$nested/sc" "$nested/sc-mcp"
printf 'license\n' >"$nested/LICENSE"
printf 'readme\n' >"$nested/README.md"
tar -C "$work/nested" -czf "$work/nested.tar.gz" "$bundle"

home="$work/home"
mkdir -p "$home/.local/bin"
HOME="$home" bash "$install_sh" "$work/nested.tar.gz"
got="$(HOME="$home" "$home/.local/bin/sc")"
case "$got" in
  'sc 0.1.6') echo "nested install after stale flat parent: ok ($got)" ;;
  *)
    echo "expected nested sc 0.1.6, got: $got" >&2
    exit 1
    ;;
esac

echo "release install staging tests passed"
