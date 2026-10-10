#!/bin/sh
# Scan release-facing text and built artifacts for private infrastructure leaks.
# Prints only path:line (or path:binary for strings hits). Never prints matched text.
set -eu

root="$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)"
cd "$root"

ALLOWED_EMAIL='322824348+mb2090@users.noreply.github.com'
dist_dir=""
release_title=""
release_body=""
tag_message=""
hits=$(mktemp)
patterns=$(mktemp)
strict_patterns=$(mktemp)
trap 'rm -f "$hits" "$patterns" "$strict_patterns"' EXIT

usage() {
  echo "usage: $0 [--dist DIR] [--release-title FILE] [--release-body FILE] [--tag-message FILE] [FILE ...]" >&2
  exit 2
}

while [ $# -gt 0 ]; do
  case "$1" in
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

cp "$patterns" "$strict_patterns"

if [ -f "${HOME:-}/.config/moonbase/release-denylist.txt" ]; then
  while IFS= read -r line || [ -n "$line" ]; do
    case "$line" in
      '' | \#*) continue ;;
    esac
    printf '%s\n' "$line" >>"$strict_patterns"
  done <"${HOME}/.config/moonbase/release-denylist.txt"
fi

if [ -n "${RELEASE_DENYLIST:-}" ]; then
  printf '%s\n' "$RELEASE_DENYLIST" >>"$strict_patterns"
fi

email_ere='[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}'

record_hit() {
  printf '%s\n' "$1" >>"$hits"
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
  file="$1"
  line_no="$2"
  line="$3"
  for addr in $(printf '%s\n' "$line" | grep -Eo "$email_ere" 2>/dev/null || true); do
    if email_is_forbidden "$addr"; then
      record_hit "$file:$line_no"
    fi
  done
}

scan_text_file() {
  file="$1"
  pattern_file="$2"
  [ -f "$file" ] || return 0
  grep_out=$(mktemp)
  if grep -Ein -f "$pattern_file" "$file" >"$grep_out" 2>/dev/null; then
    while IFS= read -r row; do
      [ -n "$row" ] || continue
      num=${row%%:*}
      record_hit "$file:$num"
    done <"$grep_out"
  fi
  rm -f "$grep_out"
  line_no=0
  while IFS= read -r line || [ -n "$line" ]; do
    line_no=$((line_no + 1))
    scan_emails_in_line "$file" "$line_no" "$line"
  done <"$file"
}

scan_binary_file() {
  file="$1"
  [ -f "$file" ] || return 0
  strings_out=$(mktemp)
  if ! strings -a "$file" >"$strings_out" 2>/dev/null; then
    rm -f "$strings_out"
    return 0
  fi
  if grep -Ein -f "$strict_patterns" "$strings_out" >/dev/null 2>&1; then
    record_hit "$file:binary"
  fi
  for addr in $(grep -Eo "$email_ere" "$strings_out" 2>/dev/null | sort -u || true); do
    if email_is_forbidden "$addr"; then
      record_hit "$file:binary"
    fi
  done
  rm -f "$strings_out"
}

scan_text_file CHANGELOG.md "$strict_patterns"
scan_text_file README.md "$patterns"
scan_text_file CONTRIBUTING.md "$patterns"
scan_text_file REVIEW_POLICY.md "$patterns"
scan_text_file SECURITY.md "$patterns"
scan_text_file SUPPORT.md "$patterns"
scan_text_file RELEASING.md "$patterns"

if [ -d docs ]; then
  find docs -type f \( -name '*.md' -o -name '*.txt' \) ! -path '*/.*' 2>/dev/null | sort | while IFS= read -r f; do
    scan_text_file "$f" "$patterns"
  done
fi

for f in release-notes release-notes.md; do
  scan_text_file "$f" "$strict_patterns"
done

[ -n "$release_title" ] && scan_text_file "$release_title" "$strict_patterns"
[ -n "$release_body" ] && scan_text_file "$release_body" "$strict_patterns"
[ -n "$tag_message" ] && scan_text_file "$tag_message" "$strict_patterns"

for extra in "$@"; do
  scan_text_file "$extra" "$strict_patterns"
done

if [ -n "$dist_dir" ] && [ -d "$dist_dir" ]; then
  find "$dist_dir" -type f ! -name SHA256SUMS 2>/dev/null | sort | while IFS= read -r artifact; do
    scan_binary_file "$artifact"
  done
fi

if [ -s "$hits" ]; then
  sort -u "$hits"
  exit 1
fi
exit 0
