#!/usr/bin/env python3
"""Fail if a release .tar.gz does not use the nested sc-vX.Y.Z-<target>/ layout."""
from __future__ import annotations

import argparse
import sys
import tarfile
from pathlib import Path

REQUIRED = frozenset({"sc", "sc-mcp", "LICENSE", "README.md"})


def validate(archive: Path) -> tuple[bool, str]:
    with tarfile.open(archive, "r:gz") as tf:
        names = [n for n in tf.getnames() if n and n != "."]
    if not names:
        return False, "archive is empty"
    prefix: str | None = None
    found: set[str] = set()
    for name in names:
        if name.startswith("._") or "/._" in name:
            continue
        parts = [p for p in name.split("/") if p]
        if not parts:
            continue
        if len(parts) == 1:
            top = parts[0]
            if top.startswith("sc-v"):
                continue
            return (
                False,
                f"flat or shallow entry {name!r} (expected sc-v*/<file>)",
            )
        top = parts[0]
        if not top.startswith("sc-v"):
            return False, f"top-level directory must be sc-v*, got {top!r}"
        if prefix is None:
            prefix = top
        elif top != prefix:
            return False, f"mixed bundle directories {prefix!r} and {top!r}"
        if len(parts) == 2:
            found.add(parts[1])
    if prefix is None:
        return False, "no sc-v* bundle directory in archive"
    missing = REQUIRED - found
    if missing:
        return False, f"missing under {prefix}/: {', '.join(sorted(missing))}"
    return True, f"ok ({prefix}, {len(names)} entries)"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive", type=Path)
    args = parser.parse_args()
    ok, msg = validate(args.archive)
    if ok:
        print(msg)
        return 0
    print(msg, file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(main())
