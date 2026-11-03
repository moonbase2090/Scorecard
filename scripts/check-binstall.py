#!/usr/bin/env python3
"""Verify both binary manifests against release archives without installing them."""

from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TARGETS = (
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "aarch64-unknown-linux-gnu",
    "x86_64-unknown-linux-gnu",
    "x86_64-pc-windows-msvc",
)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binstall", default=shutil.which("cargo-binstall"))
    args = parser.parse_args()
    if not args.binstall:
        parser.error(
            "cargo-binstall is missing; run cargo install cargo-binstall --locked"
        )
    metadata = json.loads(
        subprocess.check_output(
            ["cargo", "metadata", "--no-deps", "--locked", "--format-version", "1"],
            cwd=ROOT,
            text=True,
        )
    )
    packages = {p["name"]: p for p in metadata["packages"]}
    for name in ("sc-cli", "sc-mcp"):
        package = packages[name]
        disabled = package["metadata"]["binstall"].get("disabled-strategies", [])
        if not {"quick-install", "compile"}.issubset(disabled):
            parser.error(
                f"{name}: disable quick-install and compile in package.metadata.binstall"
            )
        binaries = [t["name"] for t in package["targets"] if "bin" in t["kind"]]
        for target in TARGETS:
            with tempfile.TemporaryDirectory(prefix="sc-binstall-") as tmp:
                destination = Path(tmp) / "bin"
                command = [
                    args.binstall,
                    "--dry-run",
                    "--no-confirm",
                    "--no-discover-github-token",
                    "--strategies",
                    "crate-meta-data",
                    "--manifest-path",
                    package["manifest_path"],
                    "--targets",
                    target,
                    "--install-path",
                    str(destination),
                    name,
                ]
                result = subprocess.run(
                    command, cwd=ROOT, capture_output=True, text=True, check=False
                )
                if result.returncode:
                    print(result.stdout + result.stderr)
                    print(
                        f"FAIL {name} {target}: check the published archive and binstall metadata; rerun python3 scripts/check-binstall.py"
                    )
                    return 1
                suffix = ".exe" if "windows" in target else ""
                if any(
                    (destination / (binary + suffix)).exists() for binary in binaries
                ):
                    raise RuntimeError(
                        "cargo-binstall installed a binary despite --dry-run"
                    )
            print(
                f"ok {name} {package['version']} {target}: archive verified, no binary installed",
                flush=True,
            )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
