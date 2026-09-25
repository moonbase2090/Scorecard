#!/usr/bin/env bats

setup() {
  # shellcheck disable=SC1091
  source "${BATS_TEST_DIRNAME}/../scripts/choose.sh"
}

@test "picks a" {
  [ "$(choose a)" = "a" ]
}
