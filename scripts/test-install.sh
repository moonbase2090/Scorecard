#!/usr/bin/env bash
# The installer must pick the archive for the host, refuse a bad checksum, and
# leave a user bin directory only after the digest matches.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
install_sh="$root/install.sh"
dash -n "$install_sh"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/tmp" "$work/files"
files="$work/files"

fakebin="$work/fakebin"
mkdir -p "$fakebin"

cat >"$fakebin/curl" <<'EOF'
#!/bin/sh
set -eu
out=
url=
while [ "$#" -gt 0 ]; do
  case "$1" in
    -o)
      out=$2
      shift 2
      ;;
    https://*)
      url=$1
      shift
      ;;
    *)
      shift
      ;;
  esac
done
if [ -z "${out}" ] || [ -z "${url}" ]; then
  echo "curl stub: missing output or url" >&2
  exit 2
fi
printf '%s\n' "$url" >>"$CURL_LOG"
case "$url" in
  */releases/latest)
    cp "$FIXTURE_ROOT/latest.json" "$out"
    ;;
  */releases/download/v*/SHA256SUMS)
    ver=$(printf '%s\n' "$url" | sed -n 's|.*/download/v\([^/]*\)/SHA256SUMS|\1|p')
    cp "$FIXTURE_ROOT/$ver/SHA256SUMS" "$out"
    ;;
  */releases/download/v*/*)
    ver=$(printf '%s\n' "$url" | sed -n 's|.*/download/v\([^/]*\)/[^/]*|\1|p')
    file=$(basename "$url")
    cp "$FIXTURE_ROOT/$ver/$file" "$out"
    ;;
  *)
    echo "curl stub: unmapped $url" >&2
    exit 22
    ;;
esac
EOF

cat >"$fakebin/uname" <<'EOF'
#!/bin/sh
set -eu
case "${1:-}" in
  -s) printf '%s\n' "$FAKE_UNAME_S" ;;
  -m) printf '%s\n' "$FAKE_UNAME_M" ;;
  *)
    echo "uname stub: unexpected $*" >&2
    exit 1
    ;;
esac
EOF
chmod 755 "$fakebin/curl" "$fakebin/uname"

write_exec() {
  local path=$1 marker=$2
  printf '#!/bin/sh\nprintf "%%s\\n" "%s"\n' "$marker" >"$path"
  chmod 755 "$path"
}

pack_tree() {
  local version=$1 target=$2 marker=$3 kind=$4 layout=$5
  local bundle="sc-v${version}-${target}"
  local src="$work/src/${version}-${target}-${layout}"
  local dest="$files/$version"
  local asset
  rm -rf "$src"
  mkdir -p "$dest"
  if [ "$kind" = zip ]; then
    asset="${bundle}.zip"
    local bin_dir
    if [ "$layout" = flat ]; then
      bin_dir=$src
    else
      bin_dir=$src/$bundle
    fi
    mkdir -p "$bin_dir"
    write_exec "$bin_dir/sc.exe" "sc:${marker}"
    if [ "$layout" != no-mcp ]; then
      write_exec "$bin_dir/sc-mcp.exe" "mcp:${marker}"
    fi
    printf 'license\n' >"$bin_dir/LICENSE"
    printf 'readme\n' >"$bin_dir/README.md"
    python3 - "$src" "$dest/$asset" <<'PY'
import sys
import zipfile
from pathlib import Path

src, out = Path(sys.argv[1]), Path(sys.argv[2])
with zipfile.ZipFile(out, "w") as zf:
    for path in sorted(p for p in src.rglob("*") if p.is_file()):
        zf.write(path, path.relative_to(src).as_posix())
PY
  else
    asset="${bundle}.tar.gz"
    local bin_dir
    if [ "$layout" = flat ]; then
      bin_dir=$src
    else
      bin_dir=$src/$bundle
    fi
    mkdir -p "$bin_dir"
    write_exec "$bin_dir/sc" "sc:${marker}"
    if [ "$layout" != no-mcp ]; then
      write_exec "$bin_dir/sc-mcp" "mcp:${marker}"
    fi
    printf 'license\n' >"$bin_dir/LICENSE"
    printf 'readme\n' >"$bin_dir/README.md"
    if [ "$layout" = flat ]; then
      tar -C "$src" -czf "$dest/$asset" sc sc-mcp LICENSE README.md
    else
      tar -C "$src" -czf "$dest/$asset" "$bundle"
    fi
  fi
}

write_sums() {
  local version=$1
  local decoy
  decoy="$(printf '%064d' 0)"
  (
    cd "$files/$version"
    {
      printf '%s  %s\n' "$decoy" "not-the-asset"
      find . -maxdepth 1 -type f ! -name SHA256SUMS -printf '%f\n' | sort | xargs sha256sum
    } >SHA256SUMS
  )
}

pack_tree 0.1.14 x86_64-unknown-linux-gnu linux-x86_64 tar nested
pack_tree 0.1.14 aarch64-unknown-linux-gnu linux-aarch64 tar nested
pack_tree 0.1.14 x86_64-apple-darwin darwin-x86_64 tar nested
pack_tree 0.1.14 aarch64-apple-darwin darwin-aarch64 tar nested
pack_tree 0.1.14 x86_64-pc-windows-msvc windows-x86_64 zip nested
write_sums 0.1.14

pack_tree 0.1.6 x86_64-unknown-linux-gnu flat-0.1.6 tar flat
write_sums 0.1.6

pack_tree 0.1.7 x86_64-unknown-linux-gnu no-mcp tar no-mcp
write_sums 0.1.7

mkdir -p "$files/9.9.9"
cp "$files/0.1.14/sc-v0.1.14-x86_64-unknown-linux-gnu.tar.gz" \
  "$files/9.9.9/sc-v9.9.9-x86_64-unknown-linux-gnu.tar.gz"
(
  cd "$files/9.9.9"
  sha256sum sc-v9.9.9-x86_64-unknown-linux-gnu.tar.gz >SHA256SUMS
  printf 'x' >>sc-v9.9.9-x86_64-unknown-linux-gnu.tar.gz
)

python3 - "$files" <<'PY'
import hashlib
import io
import tarfile
from pathlib import Path

files = Path(__import__("sys").argv[1])
dest = files / "9.9.8"
dest.mkdir(parents=True)
archive = dest / "sc-v9.9.8-x86_64-unknown-linux-gnu.tar.gz"
payload = b"#!/bin/sh\nprintf '%s\\n' 'escaped'\n"
info = tarfile.TarInfo("../../outside-sc")
info.size = len(payload)
info.mode = 0o755
with tarfile.open(archive, "w:gz") as tf:
    tf.addfile(info, io.BytesIO(payload))
digest = hashlib.sha256(archive.read_bytes()).hexdigest()
(dest / "SHA256SUMS").write_text(
    f"{digest}  {archive.name}\n",
    encoding="utf-8",
)
PY

mkdir -p "$files/9.9.7"
cp "$files/0.1.14/sc-v0.1.14-x86_64-unknown-linux-gnu.tar.gz" \
  "$files/9.9.7/sc-v9.9.7-x86_64-unknown-linux-gnu.tar.gz"
printf '%s  %s\n' "$(printf '%064d' 0)" "some-other-file.tar.gz" >"$files/9.9.7/SHA256SUMS"

printf '%s\n' '{"tag_name": "v0.1.14"}' >"$files/latest.json"

run_install() {
  local os=$1 arch=$2 home=$3 log=$4
  shift 4
  local path="$fakebin:$PATH"
  if [ "${1:-}" = "--on-path" ]; then
    path="$home/.local/bin:$path"
    shift
  fi
  mkdir -p "$home" "$work/tmp"
  : >"$log"
  env -u SC_VERSION \
    HOME="$home" \
    PATH="$path" \
    FAKE_UNAME_S="$os" \
    FAKE_UNAME_M="$arch" \
    CURL_LOG="$log" \
    FIXTURE_ROOT="$files" \
    TMPDIR="$work/tmp" \
    "$@" \
    /bin/sh "$install_sh"
}

assert_install() {
  local os=$1 arch=$2 version=$3 marker=$4 suffix=$5 target=$6
  local home=$work/home-${marker}-${arch}
  local log=$work/curl-${marker}-${arch}.log
  local out status bin mcp
  set +e
  out="$(run_install "$os" "$arch" "$home" "$log" "SC_VERSION=${version}" 2>&1)"
  status=$?
  set -e
  if [ "$status" -ne 0 ]; then
    printf '%s\n' "$out" >&2
    echo "install failed for ${os} ${arch} v${version}" >&2
    exit 1
  fi
  if [ "$suffix" = zip ]; then
    bin=sc.exe
    mcp=sc-mcp.exe
  else
    bin=sc
    mcp=sc-mcp
  fi
  [ "$(stat -c %a "$home/.local/bin/$bin")" = 755 ]
  [ "$("$home/.local/bin/$bin")" = "sc:${marker}" ]
  [ "$("$home/.local/bin/$mcp")" = "mcp:${marker}" ]
  printf '%s\n' "$out" | grep -F -q "installed ${bin} v${version} (${target}) to ${home}/.local/bin"
  printf '%s\n' "$out" | grep -F -q 'export PATH="'
  local asset="sc-v${version}-${target}.${suffix}"
  grep -F -q -x "https://github.com/moonbase2090/Scorecard/releases/download/v${version}/${asset}" "$log"
  grep -F -q -x "https://github.com/moonbase2090/Scorecard/releases/download/v${version}/SHA256SUMS" "$log"
  if grep -F 'releases/latest' "$log" >/dev/null; then
    echo "pinned install requested the latest-release API" >&2
    exit 1
  fi
  # The decoy digest is the first SHA256SUMS line. Installing still yields this marker.
  echo "ok ${os} ${arch} -> ${marker}"
}

assert_install Linux x86_64 0.1.14 linux-x86_64 tar.gz x86_64-unknown-linux-gnu
assert_install Linux amd64 0.1.14 linux-x86_64 tar.gz x86_64-unknown-linux-gnu
assert_install Linux aarch64 0.1.14 linux-aarch64 tar.gz aarch64-unknown-linux-gnu
assert_install Linux arm64 0.1.14 linux-aarch64 tar.gz aarch64-unknown-linux-gnu
assert_install Darwin x86_64 0.1.14 darwin-x86_64 tar.gz x86_64-apple-darwin
assert_install Darwin arm64 0.1.14 darwin-aarch64 tar.gz aarch64-apple-darwin
assert_install Darwin aarch64 0.1.14 darwin-aarch64 tar.gz aarch64-apple-darwin
assert_install MINGW64_NT-10.0-20348 x86_64 0.1.14 windows-x86_64 zip x86_64-pc-windows-msvc
assert_install MSYS_NT-10.0 x86_64 0.1.14 windows-x86_64 zip x86_64-pc-windows-msvc
assert_install CYGWIN_NT-10.0 x86_64 0.1.14 windows-x86_64 zip x86_64-pc-windows-msvc

home="$work/home-vprefix"
log="$work/curl-vprefix.log"
out="$(run_install Linux x86_64 "$home" "$log" SC_VERSION=v0.1.14)"
[ "$("$home/.local/bin/sc")" = "sc:linux-x86_64" ]
grep -F -q 'download/v0.1.14/' "$log"
if grep -F 'download/vv' "$log" >/dev/null; then
  echo "v prefix was applied twice" >&2
  exit 1
fi
echo "ok SC_VERSION=v0.1.14"

home="$work/home-latest"
log="$work/curl-latest.log"
out="$(run_install Linux x86_64 "$home" "$log")"
[ "$("$home/.local/bin/sc")" = "sc:linux-x86_64" ]
grep -F -q -x 'https://api.github.com/repos/moonbase2090/Scorecard/releases/latest' "$log"
grep -F -q 'download/v0.1.14/sc-v0.1.14-x86_64-unknown-linux-gnu.tar.gz' "$log"
echo "ok latest release"

home="$work/home-on-path"
log="$work/curl-on-path.log"
out="$(run_install Linux x86_64 "$home" "$log" --on-path SC_VERSION=0.1.14)"
if printf '%s\n' "$out" | grep -F 'export PATH="' >/dev/null; then
  echo "PATH hint printed when the user bin directory is already on PATH" >&2
  printf '%s\n' "$out" >&2
  exit 1
fi
echo "ok path hint hidden"

home="$work/home-flat"
log="$work/curl-flat.log"
out="$(run_install Linux x86_64 "$home" "$log" SC_VERSION=0.1.6)"
[ "$("$home/.local/bin/sc")" = "sc:flat-0.1.6" ]
[ "$("$home/.local/bin/sc-mcp")" = "mcp:flat-0.1.6" ]
echo "ok flat layout"

home="$work/home-nomcp"
log="$work/curl-nomcp.log"
out="$(run_install Linux x86_64 "$home" "$log" SC_VERSION=0.1.7)"
[ "$("$home/.local/bin/sc")" = "sc:no-mcp" ]
[ ! -e "$home/.local/bin/sc-mcp" ]
printf '%s\n' "$out" | grep -F -q 'sc-mcp was not in sc-v0.1.7-x86_64-unknown-linux-gnu.tar.gz'
echo "ok archive without sc-mcp"

expect_fail() {
  local label=$1
  local needle=$2
  shift 2
  local out status
  set +e
  out="$("$@" 2>&1)"
  status=$?
  set -e
  if [ "$status" -eq 0 ]; then
    echo "expected failure: $label" >&2
    printf '%s\n' "$out" >&2
    exit 1
  fi
  if ! printf '%s\n' "$out" | grep -F "$needle" >/dev/null; then
    echo "expected message for $label: $needle" >&2
    printf '%s\n' "$out" >&2
    exit 1
  fi
  echo "ok fail $label"
}

home="$work/home-mismatch"
log="$work/curl-mismatch.log"
expect_fail checksum 'checksum mismatch' \
  run_install Linux x86_64 "$home" "$log" SC_VERSION=9.9.9
[ ! -e "$home/.local/bin/sc" ]
[ ! -d "$home/.local/bin" ]

home="$work/home-unsafe"
log="$work/curl-unsafe.log"
expect_fail 'unsafe path' 'contains an unsafe path' \
  run_install Linux x86_64 "$home" "$log" SC_VERSION=9.9.8
[ ! -e "$home/.local/bin/sc" ]
if find "$work" -name outside-sc -print | grep -q .; then
  echo "unsafe archive member was extracted" >&2
  find "$work" -name outside-sc -print >&2
  exit 1
fi

home="$work/home-nosum"
log="$work/curl-nosum.log"
expect_fail 'missing sum' 'has no entry' \
  run_install Linux x86_64 "$home" "$log" SC_VERSION=9.9.7
[ ! -e "$home/.local/bin/sc" ]

home="$work/home-arch"
log="$work/curl-arch.log"
expect_fail 'linux arch' 'no sc release for Linux riscv64. Install from source: https://github.com/moonbase2090/Scorecard#install' \
  run_install Linux riscv64 "$home" "$log" SC_VERSION=0.1.14
if [ -s "$log" ]; then
  echo "unsupported platform still downloaded an asset" >&2
  exit 1
fi

home="$work/home-darwin-arch"
log="$work/curl-darwin-arch.log"
expect_fail 'darwin arch' 'no sc release for Darwin ppc64. Install from source: https://github.com/moonbase2090/Scorecard#install' \
  run_install Darwin ppc64 "$home" "$log" SC_VERSION=0.1.14

home="$work/home-win-arch"
log="$work/curl-win-arch.log"
expect_fail 'windows arch' 'no sc release for Windows aarch64. Install from source: https://github.com/moonbase2090/Scorecard#install' \
  run_install MINGW64_NT-10.0 aarch64 "$home" "$log" SC_VERSION=0.1.14

home="$work/home-badver"
log="$work/curl-badver.log"
expect_fail 'bad version' 'SC_VERSION is not a release version' \
  run_install Linux x86_64 "$home" "$log" SC_VERSION=../evil
[ ! -s "$log" ]

home="$work/home-nolatest"
log="$work/curl-nolatest.log"
mkdir -p "$work/empty-fixture"
expect_fail 'latest api' 'could not read the latest sc release' \
  env HOME="$home" PATH="$fakebin:$PATH" FAKE_UNAME_S=Linux FAKE_UNAME_M=x86_64 \
  CURL_LOG="$log" FIXTURE_ROOT="$work/empty-fixture" TMPDIR="$work/tmp" \
  /bin/sh "$install_sh"

mkdir -p "$work/notag"
printf '%s\n' '{}' >"$work/notag/latest.json"
home="$work/home-notag"
log="$work/curl-notag.log"
expect_fail 'empty tag' 'did not include a v* tag' \
  env HOME="$home" PATH="$fakebin:$PATH" FAKE_UNAME_S=Linux FAKE_UNAME_M=x86_64 \
  CURL_LOG="$log" FIXTURE_ROOT="$work/notag" TMPDIR="$work/tmp" \
  /bin/sh "$install_sh"

home="$work/home-nocurl"
tooldir="$work/nocurl"
mkdir -p "$tooldir"
for tool in uname mktemp mkdir sed head awk tr grep rm install find tar sha256sum python3 unzip basename; do
  ln -s "$(command -v "$tool")" "$tooldir/$tool"
done
expect_fail 'missing curl' 'curl is not installed' \
  env HOME="$home" PATH="$tooldir" FAKE_UNAME_S=Linux FAKE_UNAME_M=x86_64 \
  CURL_LOG="$work/curl-nocurl.log" FIXTURE_ROOT="$files" TMPDIR="$work/tmp" \
  SC_VERSION=0.1.14 /bin/sh "$install_sh"

home="$work/home-nosha"
tooldir="$work/nosha"
mkdir -p "$tooldir"
for tool in curl uname mktemp mkdir sed head awk tr grep rm install find tar python3 unzip basename cp; do
  if [ "$tool" = curl ] || [ "$tool" = uname ]; then
    ln -s "$fakebin/$tool" "$tooldir/$tool"
  else
    ln -s "$(command -v "$tool")" "$tooldir/$tool"
  fi
done
log="$work/curl-nosha.log"
expect_fail 'missing sha256' 'sha256sum and shasum are missing' \
  env HOME="$home" PATH="$tooldir" FAKE_UNAME_S=Linux FAKE_UNAME_M=x86_64 \
  CURL_LOG="$log" FIXTURE_ROOT="$files" TMPDIR="$work/tmp" \
  SC_VERSION=0.1.14 /bin/sh "$install_sh"
[ ! -e "$home/.local/bin/sc" ]

# python3 is preferred for zip. Hide it so the unzip listing path runs.
home="$work/home-unzip"
tooldir="$work/unzip-only"
mkdir -p "$tooldir"
for tool in curl uname mktemp mkdir sed head awk tr grep rm install find tar sha256sum unzip basename cp; do
  if [ "$tool" = curl ] || [ "$tool" = uname ]; then
    ln -s "$fakebin/$tool" "$tooldir/$tool"
  else
    ln -s "$(command -v "$tool")" "$tooldir/$tool"
  fi
done
log="$work/curl-unzip.log"
out="$(
  env HOME="$home" PATH="$tooldir" FAKE_UNAME_S=MINGW64_NT-10.0 FAKE_UNAME_M=x86_64 \
    CURL_LOG="$log" FIXTURE_ROOT="$files" TMPDIR="$work/tmp" \
    SC_VERSION=0.1.14 /bin/sh "$install_sh"
)"
[ "$("$home/.local/bin/sc.exe")" = "sc:windows-x86_64" ]
[ "$("$home/.local/bin/sc-mcp.exe")" = "mcp:windows-x86_64" ]
echo "ok unzip fallback"

home="$work/home-nohome"
log="$work/curl-nohome.log"
: >"$log"
expect_fail 'missing home' 'HOME is not set' \
  env -u HOME PATH="$fakebin:$PATH" FAKE_UNAME_S=Linux FAKE_UNAME_M=x86_64 \
  CURL_LOG="$log" FIXTURE_ROOT="$files" TMPDIR="$work/tmp" \
  SC_VERSION=0.1.14 /bin/sh "$install_sh"
[ ! -s "$log" ]

echo "install script tests passed"
