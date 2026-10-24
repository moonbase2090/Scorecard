#!/bin/sh
# SPDX-License-Identifier: MIT
set -eu
cargo test -p sc-cli --locked --features tui --lib --test tui
cargo clippy -p sc-cli --locked --features tui --all-targets -- -D warnings
