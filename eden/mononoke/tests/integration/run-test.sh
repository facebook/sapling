#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

if [ "$#" -lt 1 ]; then
    echo "This command runs individual test file"
    echo "Usage: $0 test-file.t"
    exit 1
fi
test_file="$1"
shift
dott_target="$(buck2 uquery @fbcode//mode/dev-nosan-lg "owner('$test_file')" | grep -- -dott | head -n 1)"
test_target=${dott_target%"-dott"}
echo '$' buck run @fbcode//mode/dev-nosan-lg "$test_target" -- "$(basename "$test_file")" "$@"
buck run @fbcode//mode/dev-nosan-lg "$test_target" -- "$(basename "$test_file")" "$@"
