#!/bin/sh
# Pack a release tree with a neutral owner so the archive listing does not
# record the builder's account.
set -eu

if [ "$#" -ne 3 ]; then
  echo "usage: tar-release.sh ARCHIVE CHANGE-DIR MEMBER" >&2
  exit 2
fi

archive=$1
change_dir=$2
member=$3

if tar --version 2>/dev/null | grep -q 'GNU tar'; then
  tar --owner=root --group=root -C "$change_dir" -czf "$archive" "$member"
else
  tar -C "$change_dir" -czf "$archive" --uid 0 --gid 0 --uname root --gname wheel "$member"
fi
