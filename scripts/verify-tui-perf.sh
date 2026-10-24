#!/bin/sh
# SPDX-License-Identifier: MIT
set -eu
cargo test -p sc-cli --locked --features tui --release --lib --no-run
python3 scripts/tui-perf-pty.py
