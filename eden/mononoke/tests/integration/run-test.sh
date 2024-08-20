#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

function print_help {
    echo "This command runs individual test file."
    echo
    echo "Usage: $0 [--oss | --opt | --flagfile <FLAGFILE>] test-file.t ..."
    echo
    echo "The default build mode is dev-nosan-lg."
    echo "--oss    change build mode to fbcode//mode/dev-rust-oss"
    echo "--flagfile FLAGFILE change build mode to specified one"
    exit 1
}

function fail_error {
  >&2 echo -e "$(tput setf 5)ERROR: $1$(tput sgr0)"
  exit "1"
}

if [ $# -eq 0 ]; then
    print_help
fi

mode="fbcode//mode/dev-nosan-lg"

while [[ $# -gt 0 ]]; do
  arg="$1"
  shift
  case "$arg" in
    "-h")
      print_help
      ;;
    "--help")
      print_help
      ;;
    "--flagfile")
      mode="$1"
      shift
      ;;
    "--oss")
      mode="fbcode//mode/dev-rust-oss"
      ;;
    "--opt")
      mode="fbcode//mode/opt"
      ;;
    *)
      break;
  esac
done

if [[ "$arg" != *.t ]]; then
    fail_error "the first positional argument should be a test file, not $arg"
fi

test_file="$arg"

dott_target="$(buck2 uquery -v 0 --no-interactive-console --flagfile "$mode" "owner('$test_file')" | grep -- -dott | head -n 1)"
test_target=${dott_target%"-dott"}
echo "$(tput bold)"'$' buck run --flagfile "$mode" "$test_target" -- "$(basename "$test_file")" "$@" "$(tput sgr0)"
buck run --flagfile "$mode" "$test_target" -- "$(basename "$test_file")" "$@"
