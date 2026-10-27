#!/usr/bin/env bash
set -eu

ver="${1:?usage: package-windows-release.sh VERSION}"
base="${ver%%-*}"
src="target/x86_64-pc-windows-msvc/release"

got=$("$src/sc.exe" --version | tr -d '\r')
[ "$got" = "sc $ver" ] || [ "$got" = "sc $base" ] || {
    echo "error: sc --version is '$got', expected $ver or $base" >&2
    exit 1
}

got=$("$src/sc-mcp.exe" --version | tr -d '\r')
[ "$got" = "$ver" ] || [ "$got" = "$base" ] || {
    echo "error: sc-mcp --version is '$got', expected $ver or $base" >&2
    exit 1
}

bundle="sc-v${ver}-x86_64-pc-windows-msvc"
mkdir -p "stage/$bundle" dist
cp "$src/sc.exe" "$src/sc-mcp.exe" LICENSE README.md "stage/$bundle/"
python -c "import sys, zipfile; from pathlib import Path; root = Path('stage'); zf = zipfile.ZipFile(sys.argv[1], 'w', zipfile.ZIP_DEFLATED); [zf.write(p, p.relative_to(root).as_posix()) for p in sorted(root.rglob('*')) if p.is_file()]; zf.close()" "dist/${bundle}.zip"
python scripts/check-archive-layout.py "dist/${bundle}.zip"
