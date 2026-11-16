#!/bin/sh
# SPDX-License-Identifier: MIT
set -eu
# Styled snapshots and real-binary PTY navigation, scrolling, links and inline output.
export INSTA_UPDATE=no
cargo test -p scorecard-cli --locked --features tui --lib --test tui
cargo clippy -p scorecard-cli --locked --features tui --all-targets -- -D warnings
