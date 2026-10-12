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

zip_path_fixture="$root/scripts/fixtures/release-text-zip-path.zip"
if [ -f "$zip_path_fixture" ]; then
  dist_zip="$work/dist-zip"
  mkdir -p "$dist_zip"
  cp "$zip_path_fixture" "$dist_zip/member-name.zip"
  out=$(run_mini --dist "$dist_zip" 2>&1 || true)
  if [ -z "$out" ]; then
    echo "expected zip member path leak to fail" >&2
    exit 1
  fi
  if printf '%s\n' "$out" | grep -q '/Users/'; then
    echo "zip hit output must not echo private paths" >&2
    exit 1
  fi
  echo "zip member path scan ok"
fi

pkg_path_fixture="$root/scripts/fixtures/release-text-pkg-path.pkg"
if [ -f "$pkg_path_fixture" ]; then
  dist_pkg="$work/dist-pkg-path"
  mkdir -p "$dist_pkg"
  cp "$pkg_path_fixture" "$dist_pkg/leak.pkg"
  if run_mini --dist "$dist_pkg"; then
    echo "expected pkg member path leak to fail" >&2
    exit 1
  fi
  echo "pkg member path scan ok"
fi

symlink_fixture="$root/scripts/fixtures/release-text-symlink.tar.gz"
if [ -f "$symlink_fixture" ]; then
  dist_sym="$work/dist-sym"
  mkdir -p "$dist_sym"
  cp "$symlink_fixture" "$dist_sym/symlink.tar.gz"
  if run_mini --dist "$dist_sym"; then
    echo "expected symlink tar.gz leak to fail" >&2
    exit 1
  fi
  echo "symlink archive scan ok"
fi

nest_fixture="$root/scripts/fixtures/release-text-nested.tar.gz"
if [ -f "$nest_fixture" ]; then
  dist_nest="$work/dist-nest"
  mkdir -p "$dist_nest"
  cp "$nest_fixture" "$dist_nest/nested.tar.gz"
  if run_mini --dist "$dist_nest"; then
    echo "expected nested tar.gz+zip leak to fail" >&2
    exit 1
  fi
  echo "nested archive scan ok"
fi

fixture="$root/scripts/fixtures/release-text-leak.pkg"
if [ -f "$fixture" ]; then
  cp "$fixture" "$dist/leak.pkg"
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

mpl_dist="$work/dist-mpl"
mkdir -p "$mpl_dist/bundle"
cp "$root/LICENSE" "$mpl_dist/bundle/LICENSE"
printf 'ok\n' >"$mpl_dist/bundle/README.md"
sh "$root/scripts/tar-release.sh" "$mpl_dist/mpl.tar.gz" "$mpl_dist" bundle
rm -rf "$mpl_dist/bundle"
# A word from the official license, chosen at runtime so this script does not
# carry an optional deny term.
license_term=$(sed -n '241p' "$root/LICENSE" | awk '{ print $4 }')
export RELEASE_DENYLIST="$license_term"
if run_mini --dist "$mpl_dist"; then
  :
else
  echo "expected unmodified license text in an archive to pass optional rules" >&2
  exit 1
fi
export RELEASE_DENYLIST=''
account=$(id -un)
if tar -tzvf "$mpl_dist/mpl.tar.gz" | grep -q -F -e "$account"; then
  echo "expected archive listing to omit the builder account" >&2
  exit 1
fi
echo "unmodified license and neutral owner ok"

edited="$work/edited-license"
cp "$root/LICENSE" "$edited"
printf '\nPRIVATE-NODE-TEST\n' >>"$edited"
export RELEASE_DENYLIST='PRIVATE-NODE-TEST'
if run_mini "$edited"; then
  echo "expected a changed license file to stay under optional rules" >&2
  exit 1
fi
export RELEASE_DENYLIST=''
echo "changed license still scanned ok"

echo "check-release-text self-test ok"
