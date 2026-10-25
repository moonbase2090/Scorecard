#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Run the native Windows process-tree checks and reject empty test selection."""
import os
import subprocess
import sys

TESTS = (
    "command::windows::tests::timeout_kills_a_running_grandchild_and_captures_stderr",
    "command::windows::tests::finished_parent_does_not_leave_a_grandchild_holding_output_open",
)


def main():
    if os.name != "nt":
        sys.exit("Native Windows is required. Run: py scripts/verify-windows-process-tree.py")
    for test in TESTS:
        result = subprocess.run(
            ["cargo", "test", "--locked", "-p", "sc-engines", test, "--", "--exact", "--nocapture"],
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
        )
        print(result.stdout, end="")
        if result.returncode or f"test {test} ... ok" not in result.stdout or "1 passed; 0 failed" not in result.stdout:
            sys.exit(f"Expected one passing test: {test}")
    print("Both native Windows process-tree behaviors passed.")


if __name__ == "__main__":
    main()
