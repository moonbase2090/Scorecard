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
builtin=$(mktemp)
binary_allow=$(mktemp)
binary_patterns=$(mktemp)
trap 'rm -f "$hits" "$patterns" "$builtin" "$binary_allow" "$binary_patterns"' EXIT

# Literals that ship inside the sc and sc-mcp binaries and are not private data:
# - Scorecard's own Maven option, only as a whole -D option.
# - The author credit lines in the bundled assembly crypto code.
# - A certificate authority contact address in the bundled public root store.
cat >"$binary_allow" <<'ALLOW'
s/(^|[[:space:]])-Dmaven\.repo\.local=/\1-D/g
s/ by <[A-Za-z0-9._%+-]+@openssl\.org>//g
s/[A-Za-z0-9._%+-]+@e-szigno\.hu//g
ALLOW

# Unmodified Mozilla Public License 2.0. Optional deny rules collide with
# that required text, so only the built-in rules apply when the digest matches.
MPL2_SHA256=66a3107d5ad6a058aab753eaac2047ccb2ed0e39465dd0fe5844da3e300d5172

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
} >>"$builtin"
cp "$builtin" "$patterns"

file_sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk 'NR==1 { print $1 }'
  else
    shasum -a 256 "$1" | awk 'NR==1 { print $1 }'
  fi
}

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

# Binaries use the exact GitHub token formats instead of the broad text rule.
# Scorecard's own token-prefix table (ghp_gho_ghu_...) is not a token, and no
# substring is deleted, so a real token next to it is still found.
grep -v -F '(ghp|gho|ghs|ghu|github_pat)_' "$patterns" >"$binary_patterns" || true
printf '%s\n' \
  '(ghp|gho|ghs|ghu)_[A-Za-z0-9]{36}' \
  'github_pat_[A-Za-z0-9]{22}_[A-Za-z0-9]{59}' >>"$binary_patterns"

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
    # Retina image names such as dmg-background@2x.png look like addresses.
    *@[1-9]x.png | *@[1-9]x.tiff) return 1 ;;
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
  hit_label="$1"
  text_path="$2"
  [ -f "$text_path" ] || return 0
  pattern_file="$patterns"
  digest=$(file_sha256 "$text_path" || true)
  if [ "$digest" = "$MPL2_SHA256" ]; then
    pattern_file="$builtin"
  fi
  grep_out=$(mktemp)
  grep_err=$(mktemp)
  set +e
  grep -Ein -f "$pattern_file" "$text_path" >"$grep_out" 2>"$grep_err"
  grep_status=$?
  set -e
  if [ "$grep_status" -eq 2 ]; then
    rm -f "$grep_out" "$grep_err"
    fatal_scan "pattern match failed for $hit_label"
  fi
  if [ "$grep_status" -eq 0 ]; then
    while IFS= read -r row; do
      [ -n "$row" ] || continue
      num=${row%%:*}
      record_hit "$hit_label:$num"
    done <"$grep_out"
  fi
  rm -f "$grep_out" "$grep_err"
  # One grep pass per file instead of one grep process per line.
  email_out=$(mktemp)
  grep -Eno "$email_ere" "$text_path" >"$email_out" 2>/dev/null || true
  while IFS= read -r row; do
    [ -n "$row" ] || continue
    num=${row%%:*}
    addr=${row#*:}
    if email_is_forbidden "$addr"; then
      record_hit "$hit_label:$num"
    fi
  done <"$email_out"
  rm -f "$email_out"
}

looks_like_text() {
  file="$1"
  mime=$(file -b --mime-type "$file" 2>/dev/null || true)
  case "$mime" in
    text/*) return 0 ;;
  esac
  sample=$(mktemp)
  dd if="$file" of="$sample" bs=8192 count=1 2>/dev/null || : >"$sample"
  # POSIX sh has no $'\0', so compare the size with and without NUL bytes.
  all_bytes=$(wc -c <"$sample" | tr -d ' ')
  non_nul=$(LC_ALL=C tr -d '\000' <"$sample" | wc -c | tr -d ' ')
  if [ "$all_bytes" != "$non_nul" ]; then
    rm -f "$sample"
    return 1
  fi
  rm -f "$sample"
  return 0
}

scan_binary_payload() {
  label="$1"
  file="$2"
  strings_out=$(mktemp)
  strings_raw=$(mktemp)
  if ! strings -a "$file" >"$strings_raw" 2>/dev/null; then
    rm -f "$strings_out" "$strings_raw"
    fatal_scan "strings failed for $label"
  fi
  # Remove known literals before matching. Only the literal text is removed,
  # so anything else on the same string is still scanned.
  sed -E -f "$binary_allow" "$strings_raw" >"$strings_out"
  rm -f "$strings_raw"
  set +e
  grep -Ein -f "$binary_patterns" "$strings_out" >/dev/null 2>&1
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

scan_member() {
  label="$1"
  file="$2"
  base=${file##*/}
  case "$base" in
    *.tar.gz | *.tgz)
      scan_tar_archive "$label" "$file"
      ;;
    *.pkg)
      scan_pkg_archive "$label" "$file"
      ;;
    *.dmg)
      scan_dmg_archive "$label" "$file"
      ;;
    *.zip)
      scan_zip_archive "$label" "$file"
      ;;
    *)
      scan_payload "$label" "$file"
      ;;
  esac
}

scan_symlink() {
  label="$1"
  link="$2"
  target=$(readlink "$link" 2>/dev/null) || target=""
  if [ -z "$target" ]; then
    fatal_scan "could not read symlink $label"
  fi
  probe=$(mktemp)
  printf '%s\n' "$target" >"$probe"
  scan_text_file "$label@target" "$probe"
  rm -f "$probe"
  if [ -f "$link" ]; then
    scan_member "$label" "$link"
  fi
}

scan_rel_path() {
  archive_prefix="$1"
  rel_path="$2"
  [ -n "$rel_path" ] || return 0
  probe=$(mktemp)
  printf '%s\n' "$rel_path" >"$probe"
  scan_text_file "$archive_prefix@path" "$probe"
  rm -f "$probe"
}

scan_entry() {
  label="$1"
  path="$2"
  if [ -L "$path" ]; then
    scan_symlink "$label" "$path"
  elif [ -f "$path" ]; then
    scan_member "$label" "$path"
  elif [ -d "$path" ]; then
    :
  else
    record_hit "$label:special"
  fi
}

scan_tree() {
  prefix="$1"
  dir="$2"
  skip_leaf="${3:-}"
  find "$dir" 2>/dev/null | sort | while IFS= read -r member; do
    [ "$member" = "$dir" ] && continue
    if [ -n "$skip_leaf" ] && [ "$(basename "$member")" = "$skip_leaf" ] && [ -f "$member" ]; then
      continue
    fi
    rel=${member#"$dir"/}
    rel=${rel#/}
    scan_rel_path "$prefix" "$rel"
    scan_entry "$prefix:$rel" "$member"
  done
}

list_zip_members() {
  label="$1"
  archive_path="$2"
  names=$(mktemp)
  listed=false
  if command -v unzip >/dev/null 2>&1 && unzip -Z1 "$archive_path" >"$names" 2>/dev/null; then
    listed=true
  elif command -v 7z >/dev/null 2>&1 && 7z l -ba "$archive_path" >"$names" 2>/dev/null; then
    listed=true
  fi
  if [ "$listed" = false ]; then
    rm -f "$names"
    fatal_scan "could not list $label"
  fi
  scan_text_file "$label@members" "$names"
  rm -f "$names"
}

scan_tar_archive() {
  archive_label="$1"
  archive_path="$2"
  names=$(mktemp)
  if tar -tzvf "$archive_path" >"$names" 2>/dev/null; then
    scan_text_file "$archive_label@members" "$names"
  else
    rm -f "$names"
    fatal_scan "could not list $archive_label"
  fi
  rm -f "$names"
  tmp=$(mktemp -d)
  if ! tar -xzf "$archive_path" -C "$tmp" 2>/dev/null; then
    rm -rf "$tmp"
    fatal_scan "could not unpack $archive_label"
  fi
  scan_tree "$archive_label" "$tmp"
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
  archive_label="$1"
  archive_path="$2"
  tmp=$(mktemp -d)
  unpacked=false
  if command -v bsdtar >/dev/null 2>&1 && bsdtar -xf "$archive_path" -C "$tmp" 2>/dev/null; then
    unpacked=true
  elif command -v xar >/dev/null 2>&1 && xar -xf "$archive_path" -C "$tmp" 2>/dev/null; then
    unpacked=true
  elif command -v pkgutil >/dev/null 2>&1 && pkgutil --expand-full "$archive_path" "$tmp" 2>/dev/null; then
    unpacked=true
  elif command -v pkgutil >/dev/null 2>&1 && pkgutil --expand "$archive_path" "$tmp" 2>/dev/null; then
    unpacked=true
  fi
  if [ "$unpacked" = false ]; then
    rm -rf "$tmp"
    fatal_scan "could not unpack $archive_label"
  fi
  find "$tmp" 2>/dev/null | sort | while IFS= read -r member; do
    [ "$member" = "$tmp" ] && continue
    [ "$(basename "$member")" = "Payload" ] || continue
    extract_cpio_payload "$archive_label" "$member"
  done
  scan_tree "$archive_label" "$tmp" Payload
  rm -rf "$tmp"
}

scan_dmg_archive() {
  label="$1"
  file="$2"
  tmp=$(mktemp -d)
  if command -v 7z >/dev/null 2>&1 && 7z x -y "-o$tmp" "$file" >/dev/null 2>&1; then
    scan_tree "$label" "$tmp"
    rm -rf "$tmp"
    return
  fi
  if command -v hdiutil >/dev/null 2>&1; then
    mnt="$tmp/mnt"
    mkdir -p "$mnt"
    if hdiutil attach -nobrowse -readonly -mountpoint "$mnt" "$file" >/dev/null 2>&1; then
      scan_tree "$label" "$mnt"
      hdiutil detach "$mnt" -quiet >/dev/null 2>&1 ||
        hdiutil detach "$mnt" -force -quiet >/dev/null 2>&1 ||
        true
      rm -rf "$tmp"
      return
    fi
  fi
  rm -rf "$tmp"
  fatal_scan "could not unpack $label"
}

scan_zip_archive() {
  label="$1"
  archive_path="$2"
  list_zip_members "$label" "$archive_path"
  tmp=$(mktemp -d)
  extracted=false
  if command -v 7z >/dev/null 2>&1 && 7z x -y "-o$tmp" "$archive_path" >/dev/null 2>&1; then
    extracted=true
  elif command -v unzip >/dev/null 2>&1 && unzip -qq "$archive_path" -d "$tmp" >/dev/null 2>&1; then
    extracted=true
  fi
  if [ "$extracted" = false ]; then
    rm -rf "$tmp"
    fatal_scan "could not unpack $label"
  fi
  scan_tree "$label" "$tmp"
  rm -rf "$tmp"
}

scan_dist_artifact() {
  artifact="$1"
  scan_member "$artifact" "$artifact"
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
