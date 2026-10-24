#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Run the frame budget with a draining PTY, and print its result without ANSI frames."""
import errno
import os
import pty
import re
import sys
import tempfile


def main():
    with tempfile.TemporaryFile() as transcript:
        child, master = pty.fork()
        if child == 0:
            os.execvp("cargo", [
                "cargo", "test", "-p", "sc-cli", "--locked", "--features", "tui",
                "--release", "--lib", "tui::gates::frame_budget", "--",
                "--exact", "--ignored", "--nocapture", "--test-threads=1",
            ])
        try:
            while True:
                try:
                    chunk = os.read(master, 65536)
                except OSError as error:
                    if error.errno != errno.EIO:
                        raise
                    break
                if not chunk:
                    break
                transcript.write(chunk)
        finally:
            os.close(master)
        _, status = os.waitpid(child, 0)
        code = os.waitstatus_to_exitcode(status)
        transcript.seek(0)
        output = transcript.read().decode("utf-8", errors="replace")
    summaries = re.findall(r"SC_TUI_PERF[^\r\n]*", output)
    for summary in summaries:
        print(summary)
    if code or len(summaries) != 6 or "1 passed; 0 failed" not in output:
        # Keep failures readable without printing megabytes of terminal repaint bytes.
        output = re.sub(r"\x1b\][^\x1b\x07]*(?:\x07|\x1b\\)", "", output)
        output = re.sub(r"\x1b\[[0-?]*[ -/]*[@-~]", "", output)
        print(output[-6000:], file=sys.stderr)
        print("TUI frame gate failed. Fix the reported failure and rerun scripts/verify-tui-perf.sh.", file=sys.stderr)
        return 1
    print("TUI frame gate passed: 180 verified frames, every sample <= 16 ms.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
