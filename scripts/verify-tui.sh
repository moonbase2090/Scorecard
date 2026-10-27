#!/bin/sh
# SPDX-License-Identifier: MIT
set -eu
# Styled snapshots and real-binary PTY navigation, scrolling, links and inline output.
export INSTA_UPDATE=no
cargo test -p sc-cli --locked --features tui --lib --test tui
cargo clippy -p sc-cli --locked --features tui --all-targets -- -D warnings
