#!/bin/sh
# SPDX-License-Identifier: MIT
set -eu
# Styled snapshots and real-binary PTY navigation, scrolling, links and inline output.
export INSTA_UPDATE=no
cargo test -p scorecard-cli --locked --lib --test tui
cargo clippy -p scorecard-cli --locked --all-targets -- -D warnings
