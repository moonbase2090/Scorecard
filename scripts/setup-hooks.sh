#!/bin/sh
# Point this clone at the checked-in hooks in .githooks.
set -eu
cd "$(git rev-parse --show-toplevel)"
git config core.hooksPath .githooks
echo "core.hooksPath=$(git config core.hooksPath)"
