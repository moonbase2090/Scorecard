#!/bin/sh
# Scan release-facing text and built artifacts for private infrastructure leaks.
# Prints only path:line (or path:binary). Never prints matched text.
set -eu

script_root="$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)"
root="$script_root"

ALLOWED_EMAIL='322824348+mb2090@users.noreply.github.com'
dist_dir=""
release_title=""
release_body=""
tag_message=""
hits=$(mktemp)
patterns=$(mktemp)
trap 'rm -f "$hits" "$patterns"' EXIT

usage() {
  echo "usage: $0 [--root DIR] [--dist DIR] [--release-title FILE] [--release-body FILE] [--tag-message FILE] [FILE ...]" >&2
  exit 2
}

while [ $# -gt 0 ]; do
  case "$1" in
    --root)
      root="${2:-}"
      shift 2
      ;;
    --dist)
      dist_dir="${2:-}"
      shift 2
      ;;
    --release-title)
      release_title="${2:-}"
      shift 2
      ;;
    --release-body)
      release_body="${2:-}"
      shift 2
      ;;
    --tag-message)
      tag_message="${2:-}"
      shift 2
      ;;
    -h | --help)
      usage
      ;;
    --)
      shift
      break
      ;;
    -*)
      echo "unknown option: $1" >&2
      usage
      ;;
    *)
      break
      ;;
  esac
done

cd "$root"

{
  printf '%s\n' \
    'DESKTOP-[A-Z0-9]+' \
    '[A-Za-z0-9-]+\.local\b' \
    '/Users/' \
    '/home/[a-z]' \
    'C:\\Users' \
    '-----BEGIN [A-Z ]*PRIVATE KEY-----' \
    '(ghp|gho|ghs|ghu|github_pat)_[A-Za-z0-9_]{20,}' \
    '\b(10\.[0-9]+\.[0-9]+\.[0-9]+|192\.168\.[0-9]+\.[0-9]+|172\.(1[6-9]|2[0-9]|3[01])\.[0-9]+\.[0-9]+)\b'
} >>"$patterns"

if [ -f "${HOME:-}/.config/moonbase/release-denylist.txt" ]; then
  while IFS= read -r line || [ -n "$line" ]; do
    case "$line" in
      '' | \#*) continue ;;
    esac
    printf '%s\n' "$line" >>"$patterns"
  done <"${HOME}/.config/moonbase/release-denylist.txt"
fi

if [ -n "${RELEASE_DENYLIST:-}" ]; then
  printf '%s\n' "$RELEASE_DENYLIST" >>"$patterns"
fi

validate_patterns() {
  probe=$(mktemp)
  : >"$probe"
  if grep -Ein -f "$patterns" "$probe" >/dev/null 2>&1; then
    rm -f "$probe"
    return 0
  fi
  status=$?
  rm -f "$probe"
  if [ "$status" -eq 2 ]; then
    echo "check-release-text: invalid configured pattern" >&2
    exit 2
  fi
}

validate_patterns

email_ere='[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}'

record_hit() {
  printf '%s\n' "$1" >>"$hits"
}

fatal_scan() {
  echo "check-release-text: $1" >&2
  exit 2
}

email_is_forbidden() {
  addr="$1"
  [ "$addr" = "$ALLOWED_EMAIL" ] && return 1
  case "$addr" in
    *@example.com | *@example.invalid) return 1 ;;
  esac
  return 0
}

scan_emails_in_line() {
  label="$1"
  line_no="$2"
  line="$3"
  for addr in $(printf '%s\n' "$line" | grep -Eo "$email_ere" 2>/dev/null || true); do
    if email_is_forbidden "$addr"; then
      record_hit "$label:$line_no"
    fi
  done
}

scan_text_file() {
  label="$1"
  file="$2"
  [ -f "$file" ] || return 0
  grep_out=$(mktemp)
  grep_err=$(mktemp)
  set +e
  grep -Ein -f "$patterns" "$file" >"$grep_out" 2>"$grep_err"
  grep_status=$?
  set -e
  if [ "$grep_status" -eq 2 ]; then
    rm -f "$grep_out" "$grep_err"
    fatal_scan "pattern match failed for $label"
  fi
  if [ "$grep_status" -eq 0 ]; then
    while IFS= read -r row; do
      [ -n "$row" ] || continue
      num=${row%%:*}
      record_hit "$label:$num"
    done <"$grep_out"
  fi
  rm -f "$grep_out" "$grep_err"
  line_no=0
  while IFS= read -r line || [ -n "$line" ]; do
    line_no=$((line_no + 1))
    scan_emails_in_line "$label" "$line_no" "$line"
  done <"$file"
}

looks_like_text() {
  file="$1"
  if LC_ALL=C grep -q $'\0' "$file" 2>/dev/null; then
    return 1
  fi
  return 0
}

scan_binary_payload() {
  label="$1"
  file="$2"
  strings_out=$(mktemp)
  if ! strings -a "$file" >"$strings_out" 2>/dev/null; then
    rm -f "$strings_out"
    fatal_scan "strings failed for $label"
  fi
  set +e
  grep -Ein -f "$patterns" "$strings_out" >/dev/null 2>&1
  grep_status=$?
  set -e
  if [ "$grep_status" -eq 2 ]; then
    rm -f "$strings_out"
    fatal_scan "pattern match failed for $label"
  fi
  if [ "$grep_status" -eq 0 ]; then
    record_hit "$label:binary"
  fi
  for addr in $(grep -Eo "$email_ere" "$strings_out" 2>/dev/null | sort -u || true); do
    if email_is_forbidden "$addr"; then
      record_hit "$label:binary"
    fi
  done
  rm -f "$strings_out"
}

scan_payload() {
  label="$1"
  file="$2"
  if looks_like_text "$file"; then
    scan_text_file "$label" "$file"
  else
    scan_binary_payload "$label" "$file"
  fi
}

scan_tree() {
  prefix="$1"
  dir="$2"
  find "$dir" -type f 2>/dev/null | sort | while IFS= read -r member; do
    rel=${member#"$dir"/}
    scan_payload "$prefix:$rel" "$member"
  done
}

scan_tar_archive() {
  archive="$1"
  tmp=$(mktemp -d)
  if ! tar -xzf "$archive" -C "$tmp" 2>/dev/null; then
    rm -rf "$tmp"
    fatal_scan "could not unpack $archive"
  fi
  scan_tree "$archive" "$tmp"
  rm -rf "$tmp"
}

extract_cpio_payload() {
  archive="$1"
  payload="$2"
  cpio_dir=$(mktemp -d)
  extracted=false
  if gzip -dc "$payload" 2>/dev/null | (cd "$cpio_dir" && cpio -idm 2>/dev/null); then
    extracted=true
  elif (cd "$cpio_dir" && cpio -idm <"$payload" 2>/dev/null); then
    extracted=true
  fi
  if [ "$extracted" = false ]; then
    rm -rf "$cpio_dir"
    scan_binary_payload "$archive:Payload" "$payload"
    return
  fi
  scan_tree "$archive:Payload" "$cpio_dir"
  rm -rf "$cpio_dir"
}

scan_pkg_archive() {
  archive="$1"
  tmp=$(mktemp -d)
  unpacked=false
  if command -v bsdtar >/dev/null 2>&1 && bsdtar -xf "$archive" -C "$tmp" 2>/dev/null; then
    unpacked=true
  elif command -v xar >/dev/null 2>&1 && xar -xf "$archive" -C "$tmp" 2>/dev/null; then
    unpacked=true
  elif command -v pkgutil >/dev/null 2>&1 && pkgutil --expand-full "$archive" "$tmp" 2>/dev/null; then
    unpacked=true
  elif command -v pkgutil >/dev/null 2>&1 && pkgutil --expand "$archive" "$tmp" 2>/dev/null; then
    unpacked=true
  fi
  if [ "$unpacked" = false ]; then
    rm -rf "$tmp"
    fatal_scan "could not unpack $archive"
  fi
  find "$tmp" -type f 2>/dev/null | sort | while IFS= read -r member; do
    base=$(basename "$member")
    case "$base" in
      Payload)
        extract_cpio_payload "$archive" "$member"
        ;;
      *)
        rel=${member#"$tmp"/}
        scan_payload "$archive:$rel" "$member"
        ;;
    esac
  done
  rm -rf "$tmp"
}

scan_dmg_archive() {
  archive="$1"
  tmp=$(mktemp -d)
  if command -v 7z >/dev/null 2>&1 && 7z x -y "-o$tmp" "$archive" >/dev/null 2>&1; then
    scan_tree "$archive" "$tmp"
    rm -rf "$tmp"
    return
  fi
  if command -v hdiutil >/dev/null 2>&1; then
    mnt="$tmp/mnt"
    mkdir -p "$mnt"
    if hdiutil attach -nobrowse -readonly -mountpoint "$mnt" "$archive" >/dev/null 2>&1; then
      scan_tree "$archive" "$mnt"
      hdiutil detach "$mnt" -quiet >/dev/null 2>&1 ||
        hdiutil detach "$mnt" -force -quiet >/dev/null 2>&1 ||
        true
      rm -rf "$tmp"
      return
    fi
  fi
  rm -rf "$tmp"
  fatal_scan "could not unpack $archive"
}

scan_zip_archive() {
  archive="$1"
  tmp=$(mktemp -d)
  extracted=false
  if command -v 7z >/dev/null 2>&1 && 7z x -y "-o$tmp" "$archive" >/dev/null 2>&1; then
    extracted=true
  elif command -v unzip >/dev/null 2>&1 && unzip -qq "$archive" -d "$tmp" >/dev/null 2>&1; then
    extracted=true
  fi
  if [ "$extracted" = false ]; then
    rm -rf "$tmp"
    fatal_scan "could not unpack $archive"
  fi
  scan_tree "$archive" "$tmp"
  rm -rf "$tmp"
}

scan_dist_artifact() {
  artifact="$1"
  base=${artifact##*/}
  case "$base" in
    *.tar.gz | *.tgz)
      scan_tar_archive "$artifact"
      ;;
    *.pkg)
      scan_pkg_archive "$artifact"
      ;;
    *.dmg)
      scan_dmg_archive "$artifact"
      ;;
    *.zip)
      scan_zip_archive "$artifact"
      ;;
    *)
      scan_payload "$artifact" "$artifact"
      ;;
  esac
}

scan_text_file CHANGELOG.md CHANGELOG.md
scan_text_file README.md README.md
scan_text_file CONTRIBUTING.md CONTRIBUTING.md
scan_text_file REVIEW_POLICY.md REVIEW_POLICY.md
scan_text_file SECURITY.md SECURITY.md
scan_text_file SUPPORT.md SUPPORT.md
scan_text_file RELEASING.md RELEASING.md

if [ -d docs ]; then
  find docs -type f \( -name '*.md' -o -name '*.txt' \) ! -path '*/.*' 2>/dev/null | sort | while IFS= read -r f; do
    scan_text_file "$f" "$f"
  done
fi

for f in release-notes release-notes.md; do
  scan_text_file "$f" "$f"
done

[ -n "$release_title" ] && scan_text_file "$release_title" "$release_title"
[ -n "$release_body" ] && scan_text_file "$release_body" "$release_body"
[ -n "$tag_message" ] && scan_text_file "$tag_message" "$tag_message"

for extra in "$@"; do
  scan_text_file "$extra" "$extra"
done

if [ -n "$dist_dir" ] && [ -d "$dist_dir" ]; then
  find "$dist_dir" -type f ! -name SHA256SUMS 2>/dev/null | sort | while IFS= read -r artifact; do
    scan_dist_artifact "$artifact"
  done
fi

if [ -s "$hits" ]; then
  sort -u "$hits"
  exit 1
fi
exit 0
