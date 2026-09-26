#!/bin/sh
# Fail if a Rust source file is missing the MPL-2.0 SPDX header.
# CI can run: sh scripts/check-spdx.sh
set -eu
cd "$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)"
tmp=$(mktemp)
find crates testdata -name '*.rs' ! -path '*/target/*' | sort > "$tmp"
status=0
while IFS= read -r file; do
  if [ "$(head -n 1 "$file")" != "// SPDX-License-Identifier: MPL-2.0" ]; then
    echo "missing SPDX-License-Identifier: $file"
    status=1
  fi
done < "$tmp"
rm -f "$tmp"
exit "$status"
