#!/bin/sh
# SPDX-License-Identifier: MIT
set -eu
# Includes fixed viewport snapshots, 100k-row PTY scrolling, OSC 8 targets and inline output.
cargo test -p sc-cli --locked --features tui --lib --test tui
cargo clippy -p sc-cli --locked --features tui --all-targets -- -D warnings
