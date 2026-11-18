#!/bin/sh
set -eu

version=${1:?pass the release version without v}
target=${2:?pass the native Linux target}
bundle="sc-v$version-$target"
archive="dist/$bundle.tar.gz"
source="target/$target/release"
export RUSTFLAGS="--remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=/cargo --remap-path-prefix=$(pwd -P)=/src/scorecard"

cargo build --release --locked -p scorecard-cli -p sc-mcp --target "$target"

stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
mkdir -p "$stage/$bundle" "$stage/unpacked" dist
cp "$source/sc" "$source/sc-mcp" LICENSE README.md "$stage/$bundle/"
sh scripts/tar-release.sh "$archive" "$stage" "$bundle"
python3 scripts/check-archive-layout.py "$archive"
tar -xzf "$archive" -C "$stage/unpacked"

# A prerelease tag can ship binaries that report the base Cargo version.
base=${version%%-*}
for binary in sc sc-mcp; do
    prefix=
    if [ "$binary" = sc ]; then prefix='sc '; fi
    actual=$("$stage/unpacked/$bundle/$binary" --version)
    if [ "$actual" != "$prefix$version" ] && [ "$actual" != "$prefix$base" ]; then
        printf "error: %s --version is '%s', release version is '%s'\n" "$binary" "$actual" "$version" >&2
        exit 1
    fi
    printf '%s --version: %s\n' "$binary" "$actual"
done
