#!/usr/bin/env python3
"""Check that release-facing version stamps match Cargo.toml."""

from pathlib import Path
import re
import sys


ROOT = Path(__file__).resolve().parents[1]


def section(text, name):
    match = re.search(
        rf"(?ms)^\[{re.escape(name)}\]\s*\n(.*?)(?=^\[|\Z)", text
    )
    if not match:
        raise ValueError(f"missing [{name}] section")
    return match.group(1)


def field(text, key):
    match = re.search(rf'(?m)^{re.escape(key)} = "([^"]+)"$', text)
    if not match:
        raise ValueError(f"missing {key} field")
    return match.group(1)


def main():
    manifest = (ROOT / "Cargo.toml").read_text()
    version = field(section(manifest, "workspace.package"), "version")
    workspace_names = set()

    for path in (ROOT / "crates").glob("*/Cargo.toml"):
        package = section(path.read_text(), "package")
        workspace_names.add(field(package, "name"))

    lock = (ROOT / "Cargo.lock").read_text()
    locked = {}
    for block in re.findall(
        r"(?ms)^\[\[package\]\]\s*\n(.*?)(?=^\[\[package\]\]|\Z)",
        lock,
    ):
        name_match = re.search(r'(?m)^name = "([^"]+)"$', block)
        version_match = re.search(r'(?m)^version = "([^"]+)"$', block)
        if name_match and version_match and name_match.group(1) in workspace_names:
            locked[name_match.group(1)] = version_match.group(1)

    errors = []
    for name in sorted(workspace_names):
        if locked.get(name) != version:
            errors.append(f"Cargo.lock: {name} is {locked.get(name)!r}, expected {version}")

    stamp_files = [
        ROOT / "README.md",
        ROOT / "action/action.yml",
        ROOT / "packaging/INSTALL.txt",
        *sorted((ROOT / "examples/terminal").glob("*.ansi")),
        *sorted((ROOT / "examples/terminal").glob("*.txt")),
        *sorted((ROOT / "crates/sc-cli/tests/golden").glob("*.pretty.txt")),
    ]

    for path in stamp_files:
        text = path.read_text()
        if path.name == "README.md":
            valid = re.search(rf"(?m)^sc {re.escape(version)}  testdata/good_crate\b", text)
        elif path.name == "action.yml":
            valid = f"Release to install, such as {version}." in text
        elif path.name == "INSTALL.txt":
            valid = text.startswith(f"Scorecard {version} ")
        else:
            valid = text.startswith(f"sc {version} ")
        if not valid:
            errors.append(f"{path.relative_to(ROOT)}: version stamp does not match {version}")

    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1

    print(f"Cargo.lock and {len(stamp_files)} release-facing files use {version}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
