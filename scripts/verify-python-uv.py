#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Exercise Python runner selection with real uv projects and the built CLI."""

import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path


def main():
    binary = Path(sys.argv[1] if len(sys.argv) > 1 else "target/debug/sc").resolve()
    cases = [
        (
            "dev-group",
            '[dependency-groups]\ndev = ["pytest", "attrs"]\n',
            "--group dev",
        ),
        (
            "test-group",
            '[dependency-groups]\ntest = ["pytest", "attrs"]\n',
            "--group test",
        ),
        (
            "dev-extra",
            '[project.optional-dependencies]\ndev = ["pytest", "attrs"]\n',
            "--extra dev",
        ),
        (
            "test-extra",
            '[project.optional-dependencies]\ntest = ["pytest", "attrs"]\n',
            "--extra test",
        ),
        ("no-extra", "", None),
    ]
    env = {**os.environ, "SCORECARD_NO_AGENT_SKILLS": "1"}
    with tempfile.TemporaryDirectory(prefix="sc-uv-verify-") as tmp:
        for name, section, flag in cases:
            root = Path(tmp) / name
            root.mkdir()
            deps = '["pytest", "attrs"]' if flag is None else "[]"
            (root / "pyproject.toml").write_text(
                '[project]\nname = "runner-fixture"\nversion = "0.0.0"\n'
                f'requires-python = ">=3.11"\ndependencies = {deps}\n{section}'
            )
            (root / "tests").mkdir()
            (root / "tests/test_app.py").write_text(
                "import attrs\n\n\ndef test_attrs():\n"
                '    assert attrs.asdict(attrs.make_class("Value", {"value": attrs.field()})(7)) == {"value": 7}\n'
            )
            subprocess.run(
                ["uv", "lock"], cwd=root, env=env, check=True, capture_output=True
            )
            if name == "dev-group":
                before = subprocess.run(
                    [
                        "uv",
                        "run",
                        "--extra",
                        "dev",
                        "--with",
                        "pytest-cov",
                        "pytest",
                        "-q",
                    ],
                    cwd=root,
                    env=env,
                    capture_output=True,
                    text=True,
                    check=False,
                )
                assert (
                    before.returncode != 0
                    and "Extra `dev` is not defined" in before.stderr
                ), before.stderr
                print(
                    "before: --extra dev fails because dev is a dependency group",
                    flush=True,
                )
            result = subprocess.run(
                [
                    str(binary),
                    "analyze",
                    str(root),
                    "--pack",
                    "python",
                    "--format",
                    "json",
                ],
                env=env,
                capture_output=True,
                text=True,
                check=False,
            )
            report = json.loads(result.stdout)
            gate = next(g for g in report["gates"] if g["id"] == "tests")
            assert gate["enforced"] and gate["pass"], (name, gate, result.stderr)
            run = next(r for r in report["runs"] if r["engine"] == "tests")
            assert run["exit_code"] == 0, run
            assert (
                flag in run["command"]
                if flag
                else "--extra" not in run["command"] and "--group" not in run["command"]
            ), run
            command = run["command"].replace(str(root.resolve()), "<fixture>")
            print(f"{name}: tests pass, exit 0; {command}", flush=True)
            # Exercise coverage.py's fallback command with the same selected flags.
            runner = [
                "uv",
                "run",
                *(flag.split() if flag else []),
                "--with",
                "coverage",
                "coverage",
            ]
            fallback = subprocess.run(
                [*runner, "run", "-m", "pytest", "-q"],
                cwd=root,
                env=env,
                check=True,
                capture_output=True,
                text=True,
            )
            subprocess.run(
                [*runner, "json", "-o", "coverage.json"],
                cwd=root,
                env=env,
                check=True,
                capture_output=True,
            )
            print(f"{name}: coverage fallback; {fallback.stdout.strip()}", flush=True)


if __name__ == "__main__":
    main()
