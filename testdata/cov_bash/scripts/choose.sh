#!/bin/bash
choose() {
  if [ "$1" = "a" ]; then
    echo a
  else
    echo b
  fi
}
if [ "${BASH_SOURCE[0]}" = "$0" ]; then
  choose "$@"
fi
