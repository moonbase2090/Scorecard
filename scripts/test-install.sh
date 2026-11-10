#!/usr/bin/env bash
# install.sh must select the archive for the OS and CPU, reject a checksum
# that does not match that archive, and install into the requested directory.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
install_sh=$root/install.sh
work=$(mktemp -d)
server_pid=
OUTPUT=
STATUS=0

cleanup() {
  if [ -n "${server_pid:-}" ]; then
    kill "$server_pid" 2>/dev/null || true
    wait "$server_pid" 2>/dev/null || true
  fi
  rm -rf "$work"
}
trap cleanup EXIT

sh -n "$install_sh"

srv=$work/srv
mkdir -p "$srv"
ORIG_PATH=$PATH

sha256_file() {
  python3 -c 'import hashlib,sys; print(hashlib.sha256(open(sys.argv[1],"rb").read()).hexdigest())' "$1"
}

write_stub() {
  path=$1
  shift
  {
    printf '%s\n' '#!/bin/sh'
    printf '%s\n' "$@"
  } > "$path"
  chmod 755 "$path"
}

pack_nested() {
  ver=$1
  triple=$2
  dest_dir=$3
  bundle="sc-v${ver}-${triple}"
  stage=$work/pack/$bundle
  rm -rf "$stage"
  mkdir -p "$stage"
  write_stub "$stage/sc" "printf 'sc %s\\n' '$triple'"
  write_stub "$stage/sc-mcp" "printf 'mcp %s\\n' '$triple'"
  tar -C "$work/pack" -czf "$dest_dir/${bundle}.tar.gz" "$bundle"
}

pack_flat() {
  dest_dir=$1
  name=$2
  stage=$work/flatpack
  rm -rf "$stage"
  mkdir -p "$stage"
  write_stub "$stage/sc" "printf 'sc flat\\n'"
  write_stub "$stage/sc-mcp" "printf 'mcp flat\\n'"
  tar -C "$stage" -czf "$dest_dir/$name" sc sc-mcp
}

pack_broken() {
  dest_dir=$1
  name=$2
  marker=$3
  stage=$work/brokenpack
  rm -rf "$stage"
  mkdir -p "$stage"
  write_stub "$stage/sc" "printf ran >> '$marker'" "exit 3"
  write_stub "$stage/sc-mcp" "exit 0"
  tar -C "$stage" -czf "$dest_dir/$name" sc sc-mcp
}

write_sums() {
  dir=$1
  {
    printf '%s  %s\r\n' '0000000000000000000000000000000000000000000000000000000000000000' 'not-the-archive.txt'
    for archive in "$dir"/*.tar.gz; do
      base=$(basename "$archive")
      hash=$(sha256_file "$archive" | tr 'a-f' 'A-F')
      printf '%s  %s\r\n' "$hash" "$base"
    done
  } > "$dir/SHA256SUMS"
}

good=$srv/v0.1.14
mkdir -p "$good"
for triple in aarch64-apple-darwin x86_64-apple-darwin aarch64-unknown-linux-gnu x86_64-unknown-linux-gnu; do
  pack_nested 0.1.14 "$triple" "$good"
done
write_sums "$good"

flat=$srv/v0.1.6
mkdir -p "$flat"
pack_flat "$flat" sc-v0.1.6-x86_64-unknown-linux-gnu.tar.gz
write_sums "$flat"

swap=$srv/swap
mkdir -p "$swap"
cp "$good/sc-v0.1.14-aarch64-unknown-linux-gnu.tar.gz" "$swap/sc-v0.1.14-x86_64-unknown-linux-gnu.tar.gz"
x86_hash=$(sha256_file "$good/sc-v0.1.14-x86_64-unknown-linux-gnu.tar.gz")
printf '%s  %s\n' "$x86_hash" 'sc-v0.1.14-x86_64-unknown-linux-gnu.tar.gz' > "$swap/SHA256SUMS"

miss=$srv/miss
mkdir -p "$miss"
cp "$good/sc-v0.1.14-x86_64-unknown-linux-gnu.tar.gz" "$miss/"
printf '%s  %s\n' '0000000000000000000000000000000000000000000000000000000000000000' 'not-the-archive.txt' > "$miss/SHA256SUMS"

marker=$work/ran-marker
broken=$srv/broken
mkdir -p "$broken"
pack_broken "$broken" sc-v0.1.14-x86_64-unknown-linux-gnu.tar.gz "$marker"
printf '%s  %s\n' "$(sha256_file "$broken/sc-v0.1.14-x86_64-unknown-linux-gnu.tar.gz")" 'sc-v0.1.14-x86_64-unknown-linux-gnu.tar.gz' > "$broken/SHA256SUMS"

uname_dir=$work/bin
mkdir -p "$uname_dir"
cat > "$uname_dir/uname" <<'EOF'
#!/bin/sh
case "${1-}" in
  -s) printf '%s\n' "${FAKE_UNAME_S:?}" ;;
  -m) printf '%s\n' "${FAKE_UNAME_M:?}" ;;
  *) printf 'unexpected uname args: %s\n' "$*" >&2; exit 1 ;;
esac
EOF
chmod 755 "$uname_dir/uname"

port=$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()')
python3 -m http.server --bind 127.0.0.1 --directory "$srv" "$port" >"$work/http.log" 2>&1 &
server_pid=$!
ready=0
i=0
while [ "$i" -lt 30 ]; do
  if curl -fsS "http://127.0.0.1:${port}/v0.1.14/SHA256SUMS" >/dev/null 2>&1; then
    ready=1
    break
  fi
  i=$((i + 1))
  sleep 0.1
done
if [ "$ready" != 1 ]; then
  echo "fixture server did not start" >&2
  cat "$work/http.log" >&2
  exit 1
fi

invoke() {
  os=$1
  arch=$2
  ver=$3
  base=$4
  home=$5
  dest_mode=$6
  dest_value=${7-}
  path_prefix=${8-}
  set +e
  if [ "$dest_mode" = unset ]; then
    OUTPUT=$(
      env -u SC_INSTALL_DIR \
        HOME="$home" \
        PATH="${path_prefix:+$path_prefix:}$uname_dir:$ORIG_PATH" \
        FAKE_UNAME_S="$os" \
        FAKE_UNAME_M="$arch" \
        SC_VERSION="$ver" \
        SC_RELEASE_BASE="$base" \
        sh "$install_sh" 2>&1
    )
  else
    OUTPUT=$(
      env \
        HOME="$home" \
        PATH="${path_prefix:+$path_prefix:}$uname_dir:$ORIG_PATH" \
        FAKE_UNAME_S="$os" \
        FAKE_UNAME_M="$arch" \
        SC_VERSION="$ver" \
        SC_RELEASE_BASE="$base" \
        SC_INSTALL_DIR="$dest_value" \
        sh "$install_sh" 2>&1
    )
  fi
  STATUS=$?
  set -e
}

require_ok() {
  if [ "$STATUS" -ne 0 ]; then
    printf 'FAIL: %s\n%s\n' "$1" "$OUTPUT" >&2
    exit 1
  fi
}

require_fail() {
  if [ "$STATUS" -eq 0 ]; then
    printf 'FAIL: %s succeeded\n%s\n' "$1" "$OUTPUT" >&2
    exit 1
  fi
  case $OUTPUT in
    *"$2"*) ;;
    *)
      printf 'FAIL: %s missing "%s"\n%s\n' "$1" "$2" "$OUTPUT" >&2
      exit 1
      ;;
  esac
}

base="http://127.0.0.1:${port}/v0.1.14"
home=$work/home
mkdir -p "$home"

# Trailing slash and a leading v are accepted. Each host maps to its own archive.
invoke Darwin arm64 v0.1.14 "${base}/" "$home" dir "$work/dest-darwin-arm"
require_ok "Darwin arm64"
[ "$("$work/dest-darwin-arm/sc")" = "sc aarch64-apple-darwin" ]
[ "$("$work/dest-darwin-arm/sc-mcp")" = "mcp aarch64-apple-darwin" ]
[ ! -e "$home/.local/bin/sc" ]
case $OUTPUT in
  *"export PATH=\"$work/dest-darwin-arm:\$PATH\""*) ;;
  *)
    printf 'FAIL: missing PATH hint\n%s\n' "$OUTPUT" >&2
    exit 1
    ;;
esac

while read -r os arch triple; do
  [ -n "$os" ] || continue
  dest=$work/dest-$os-$arch
  invoke "$os" "$arch" 0.1.14 "$base" "$home" dir "$dest"
  require_ok "$os $arch"
  [ "$("$dest/sc")" = "sc $triple" ]
  [ "$("$dest/sc-mcp")" = "mcp $triple" ]
done <<'EOF'
Darwin aarch64 aarch64-apple-darwin
Darwin x86_64 x86_64-apple-darwin
Linux aarch64 aarch64-unknown-linux-gnu
Linux arm64 aarch64-unknown-linux-gnu
Linux x86_64 x86_64-unknown-linux-gnu
Linux amd64 x86_64-unknown-linux-gnu
EOF

invoke Linux x86_64 0.1.14 "$base" "$home" unset "" "$work/on-path"
on_path=$home/.local/bin
require_ok "default directory"
[ -x "$on_path/sc" ]
[ "$("$on_path/sc")" = "sc x86_64-unknown-linux-gnu" ]

mkdir -p "$work/on-path"
invoke Linux x86_64 0.1.14 "$base" "$home" dir "$work/on-path" "$work/on-path"
require_ok "directory already on PATH"
case $OUTPUT in
  *"not on PATH"*)
    printf 'FAIL: PATH hint printed for a directory already on PATH\n%s\n' "$OUTPUT" >&2
    exit 1
    ;;
esac

invoke Linux x86_64 0.1.6 "http://127.0.0.1:${port}/v0.1.6" "$home" dir "$work/dest-flat"
require_ok "flat archive"
[ "$("$work/dest-flat/sc")" = "sc flat" ]
[ "$("$work/dest-flat/sc-mcp")" = "mcp flat" ]

invoke Linux x86_64 0.1.14 "http://127.0.0.1:${port}/swap" "$home" dir "$work/dest-swap"
require_fail "checksum mismatch" "checksum mismatch"
[ ! -e "$work/dest-swap/sc" ]
case $OUTPUT in
  *"expected $x86_hash"*) ;;
  *)
    printf 'FAIL: checksum error did not name the expected hash\n%s\n' "$OUTPUT" >&2
    exit 1
    ;;
esac

invoke Linux x86_64 0.1.14 "http://127.0.0.1:${port}/miss" "$home" dir "$work/dest-miss"
require_fail "missing checksum entry" "no entry for"
[ ! -e "$work/dest-miss/sc" ]

rm -f "$marker"
invoke Linux x86_64 0.1.14 "http://127.0.0.1:${port}/broken" "$home" dir "$work/dest-broken"
require_fail "binary does not run" "did not run"
[ ! -e "$work/dest-broken/sc" ]
[ ! -e "$work/dest-broken/sc-mcp" ]
[ -s "$marker" ]

printf x > "$work/not-a-dir"
invoke Linux x86_64 0.1.14 "$base" "$home" dir "$work/not-a-dir/bin"
require_fail "unwritable destination" "cannot create"
case $OUTPUT in
  *SC_INSTALL_DIR*) ;;
  *)
    printf 'FAIL: create error did not name SC_INSTALL_DIR\n%s\n' "$OUTPUT" >&2
    exit 1
    ;;
esac

invoke Linux riscv64 0.1.14 "http://127.0.0.1:1/unused" "$home" dir "$work/dest-riscv"
require_fail "unsupported arch" "riscv64"
case $OUTPUT in
  *"not a published build"*) ;;
  *)
    printf 'FAIL: unsupported arch was treated as a download error\n%s\n' "$OUTPUT" >&2
    exit 1
    ;;
esac
case $OUTPUT in
  *"could not download"*)
    printf 'FAIL: unsupported arch tried the network\n%s\n' "$OUTPUT" >&2
    exit 1
    ;;
esac

invoke MINGW64_NT-10.0 x86_64 0.1.14 "http://127.0.0.1:1/unused" "$home" dir "$work/dest-win"
require_fail "windows" "x86_64-pc-windows-msvc.zip"

set +e
OUTPUT=$(
  env -u HOME -u SC_INSTALL_DIR \
    PATH="$uname_dir:$ORIG_PATH" \
    FAKE_UNAME_S=Linux \
    FAKE_UNAME_M=x86_64 \
    SC_VERSION=0.1.14 \
    SC_RELEASE_BASE="http://127.0.0.1:1/unused" \
    sh "$install_sh" 2>&1
)
STATUS=$?
set -e
require_fail "HOME unset" "SC_INSTALL_DIR"
case $OUTPUT in
  *"could not download"*)
    printf 'FAIL: missing HOME tried the network\n%s\n' "$OUTPUT" >&2
    exit 1
    ;;
esac

set +e
OUTPUT=$(
  env -u SC_RELEASE_BASE \
    HOME="$home" \
    PATH="$uname_dir:$ORIG_PATH" \
    FAKE_UNAME_S=Linux \
    FAKE_UNAME_M=x86_64 \
    SC_REPO='not a repo' \
    SC_VERSION=0.1.14 \
    SC_INSTALL_DIR="$work/dest-repo" \
    sh "$install_sh" 2>&1
)
STATUS=$?
set -e
require_fail "bad SC_REPO" "SC_REPO"
case $OUTPUT in
  *"could not download"*)
    printf 'FAIL: bad SC_REPO tried the network\n%s\n' "$OUTPUT" >&2
    exit 1
    ;;
esac

echo "install.sh tests passed"
