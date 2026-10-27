#!/usr/bin/env bash
# Failing-first check: flat legacy tarballs fail; nested sc-v*/ layout passes.
set -euo pipefail
export COPYFILE_DISABLE=1
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
py=python3
check="$root/scripts/check-archive-layout.py"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Synthetic flat layout (like v0.1.6 releases) must fail.
flat="$work/flat"
mkdir -p "$flat"
printf 'x\n' >"$flat/sc"
printf 'x\n' >"$flat/sc-mcp"
printf 'license\n' >"$flat/LICENSE"
printf 'readme\n' >"$flat/README.md"
tar -C "$flat" -czf "$work/legacy-flat.tar.gz" sc sc-mcp LICENSE README.md
if "$py" "$check" "$work/legacy-flat.tar.gz"; then
  echo "expected flat archive to fail layout check" >&2
  exit 1
fi
echo "legacy-flat.tar.gz: rejected (ok)"

# Nested layout must pass.
bundle="sc-v9.9.9-x86_64-unknown-linux-gnu"
nested="$work/nested/$bundle"
mkdir -p "$nested"
cp "$flat/sc" "$flat/sc-mcp" "$flat/LICENSE" "$flat/README.md" "$nested/"
tar -C "$work/nested" -czf "$work/nested.tar.gz" "$bundle"
"$py" "$check" "$work/nested.tar.gz"
echo "nested.tar.gz: accepted (ok)"

# Windows zip: nested sc-v*/ with .exe names passes; flat or tar names fail.
wbundle="sc-v9.9.9-x86_64-pc-windows-msvc"
wnested="$work/win/$wbundle"
mkdir -p "$wnested"
cp "$flat/sc" "$wnested/sc.exe"
cp "$flat/sc-mcp" "$wnested/sc-mcp.exe"
cp "$flat/LICENSE" "$flat/README.md" "$wnested/"
"$py" - "$work/win" "$work/win.zip" "$wbundle" <<'PY'
import sys, zipfile
from pathlib import Path
root, out, bundle = Path(sys.argv[1]), sys.argv[2], sys.argv[3]
with zipfile.ZipFile(out, "w") as zf:
    for path in sorted((root / bundle).iterdir()):
        zf.write(path, f"{bundle}/{path.name}")
PY
"$py" "$check" "$work/win.zip"
echo "win.zip: accepted (ok)"
"$py" - "$work/flat" "$work/win-bad.zip" <<'PY'
import sys, zipfile
from pathlib import Path
with zipfile.ZipFile(sys.argv[2], "w") as zf:
    for path in sorted(Path(sys.argv[1]).iterdir()):
        zf.write(path, path.name)
PY
if "$py" "$check" "$work/win-bad.zip"; then
  echo "expected flat zip to fail layout check" >&2
  exit 1
fi
echo "win-bad.zip: rejected (ok)"
mkdir -p "$work/win-sh/$wbundle"
cp "$flat/sc" "$flat/sc-mcp" "$flat/LICENSE" "$flat/README.md" "$work/win-sh/$wbundle/"
"$py" - "$work/win-sh" "$work/win-sh.zip" "$wbundle" <<'PY'
import sys, zipfile
from pathlib import Path
root, out, bundle = Path(sys.argv[1]), sys.argv[2], sys.argv[3]
with zipfile.ZipFile(out, "w") as zf:
    for path in sorted((root / bundle).iterdir()):
        zf.write(path, f"{bundle}/{path.name}")
PY
if "$py" "$check" "$work/win-sh.zip"; then
  echo "expected zip without .exe names to fail layout check" >&2
  exit 1
fi
echo "win-sh.zip: rejected (ok)"

# Published v0.1.6 asset is flat when network is available (optional).
if [ "${SCORECARD_TEST_LEGACY_RELEASE:-1}" = 1 ]; then
  url="https://github.com/moonbase2090/Scorecard/releases/download/v0.1.6/sc-v0.1.6-x86_64-unknown-linux-gnu.tar.gz"
  if curl -fsSL -o "$work/v016.tar.gz" "$url"; then
    if "$py" "$check" "$work/v016.tar.gz"; then
      echo "expected v0.1.6 release tarball to fail layout check" >&2
      exit 1
    fi
    echo "v0.1.6 release tarball: rejected (ok)"
  else
    echo "skip: could not download v0.1.6 tarball"
  fi
fi

echo "archive layout tests passed"
