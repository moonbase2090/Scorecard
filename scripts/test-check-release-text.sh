#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
check="$root/scripts/check-release-text.sh"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

export RELEASE_DENYLIST=''
HOME="$work"

mkdir -p "$work/scripts" "$work/fixture"
cp "$check" "$work/scripts/check-release-text.sh"
mini="$work/fixture"
printf 'ok\n' >"$mini/CHANGELOG.md"
printf 'ok\n' >"$mini/README.md"
printf 'ok\n' >"$mini/RELEASING.md"

run_mini() {
  HOME="$work" RELEASE_DENYLIST="${RELEASE_DENYLIST:-}" sh "$work/scripts/check-release-text.sh" --root "$mini" "$@"
}

run_mini
echo "clean fixture ok"

printf 'DESKTOP-SECRET01\n' >"$mini/bad.txt"
if run_mini "$mini/bad.txt"; then
  echo "expected DESKTOP pattern to fail" >&2
  exit 1
fi
echo "generic marker ok"

printf 'PRIVATE-NODE-TEST\n' >"$mini/README.md"
export RELEASE_DENYLIST='PRIVATE-NODE-TEST'
if run_mini; then
  echo "expected RELEASE_DENYLIST to match README" >&2
  exit 1
fi
export RELEASE_DENYLIST=''
echo "denylist on README ok"

export RELEASE_DENYLIST='('
if run_mini 2>/dev/null; then
  echo "expected invalid RELEASE_DENYLIST to exit 2" >&2
  exit 1
fi
export RELEASE_DENYLIST=''
echo "invalid denylist rejected ok"

dist="$work/dist"
mkdir -p "$dist/bundle"
printf '/Users/privateuser/secret\n' >"$dist/bundle/leak.txt"
tar -C "$dist" -czf "$dist/evil.tar.gz" bundle
rm -rf "$dist/bundle"
if run_mini --dist "$dist"; then
  echo "expected tar.gz member leak to fail" >&2
  exit 1
fi
echo "tar.gz unpack scan ok"

if command -v xar >/dev/null 2>&1; then
  pkg_src=$(mktemp -d)
  printf '/Users/pkgleak\n' >"$pkg_src/note.txt"
  (cd "$pkg_src" && xar -cf "$dist/leak.pkg" note.txt)
  rm -rf "$pkg_src"
  if run_mini --dist "$dist"; then
    echo "expected pkg member leak to fail" >&2
    exit 1
  fi
  rm -f "$dist/leak.pkg"
  echo "pkg unpack scan ok"
fi

dist_bad="$work/dist-bad"
mkdir -p "$dist_bad"
printf 'x' >"$dist_bad/bad.pkg"
set +e
run_mini --dist "$dist_bad" >/dev/null 2>&1
bad_status=$?
set -e
if [ "$bad_status" -ne 2 ]; then
  echo "expected corrupt pkg to exit 2, got $bad_status" >&2
  exit 1
fi
echo "corrupt pkg rejected ok"

echo "check-release-text self-test ok"
