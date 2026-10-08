#!/usr/bin/env bash
# Failing-first: reused $RUNNER_TEMP/sc-install shadows nested bundles; fresh mktemp does not.
set -euo pipefail
export COPYFILE_DISABLE=1
root="$(cd "$(dirname "$0")/.." && pwd)"
install_sh="$root/scripts/install-release-tarball.sh"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

runner_temp="$work/runner-temp"
mkdir -p "$runner_temp"

bundle="sc-v0.1.6-x86_64-unknown-linux-gnu"
nested="$work/nested/$bundle"
mkdir -p "$nested"
printf '#!/bin/sh\necho sc 0.1.6\n' >"$nested/sc"
printf '#!/bin/sh\necho mcp\n' >"$nested/sc-mcp"
chmod +x "$nested/sc" "$nested/sc-mcp"
printf 'license\n' >"$nested/LICENSE"
printf 'readme\n' >"$nested/README.md"
tar -C "$work/nested" -czf "$work/nested.tar.gz" "$bundle"

# Legacy flat install left sc directly in a reused staging directory (pre-fix docs).
stale="$runner_temp/sc-install"
mkdir -p "$stale"
printf '#!/bin/sh\necho sc 0.1.5\n' >"$stale/sc"
chmod +x "$stale/sc"

install_reused_stage_dir() {
  local archive="$1"
  local stage="${RUNNER_TEMP}/sc-install"
  mkdir -p "$stage"
  cp "$archive" "$stage/scorecard.tar.gz"
  tar -xzf "$stage/scorecard.tar.gz" -C "$stage"
  local root="$stage"
  if [ -f "$stage/sc" ]; then
    :
  elif comp="$(find "$stage" -maxdepth 1 -type d -name 'sc-v*' | head -1)" && [ -n "$comp" ]; then
    root="$comp"
  else
    echo "release archive has no sc binary" >&2
    return 1
  fi
  mkdir -p "$HOME/.local/bin"
  install -m 755 "$root/sc" "$HOME/.local/bin/sc"
  if [ -f "$root/sc-mcp" ]; then
    install -m 755 "$root/sc-mcp" "$HOME/.local/bin/sc-mcp"
  fi
}

home_bug="$work/home-bug"
mkdir -p "$home_bug/.local/bin"
RUNNER_TEMP="$runner_temp" HOME="$home_bug" install_reused_stage_dir "$work/nested.tar.gz"
buggy="$(HOME="$home_bug" "$home_bug/.local/bin/sc")"
case "$buggy" in
  'sc 0.1.5') echo "reused staging dir: reproduces stale flat sc ($buggy)" ;;
  *)
    echo "expected reused staging to install stale sc 0.1.5, got: $buggy" >&2
    exit 1
    ;;
esac

home_fix="$work/home-fix"
mkdir -p "$home_fix/.local/bin"
RUNNER_TEMP="$runner_temp" HOME="$home_fix" bash "$install_sh" "$work/nested.tar.gz"
fixed="$(HOME="$home_fix" "$home_fix/.local/bin/sc")"
case "$fixed" in
  'sc 0.1.6') echo "fresh mktemp under RUNNER_TEMP: ok ($fixed)" ;;
  *)
    echo "expected fresh staging to install nested sc 0.1.6, got: $fixed" >&2
    exit 1
    ;;
esac

echo "release install staging tests passed"
