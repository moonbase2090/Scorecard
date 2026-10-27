#!/bin/sh
set -eu
git config core.hooksPath .githooks
printf '%s\n' 'Installed pre-commit formatting hook.'
