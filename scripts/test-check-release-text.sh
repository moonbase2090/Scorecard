#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
check="$root/scripts/check-release-text.sh"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

export RELEASE_DENYLIST=''

printf 'ok\n' >"$work/clean.txt"
HOME="$work" sh "$check" "$work/clean.txt"

printf 'DESKTOP-SECRET01\n' >"$work/bad.txt"
if HOME="$work" sh "$check" "$work/bad.txt"; then
  echo "expected DESKTOP pattern to fail" >&2
  exit 1
fi
echo "check-release-text self-test ok"
