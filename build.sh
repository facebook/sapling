#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

TOOLCHAIN_DIR=/opt/rh/devtoolset-8/root/usr/bin
if [[ -d "$TOOLCHAIN_DIR" ]]; then
    PATH="$TOOLCHAIN_DIR:$PATH"
fi

SCRIPT_DIR=$(dirname "${BASH_SOURCE[0]}")
GETDEPS_PATHS=(
    "$SCRIPT_DIR/build/fbcode_builder/getdeps.py"
    "$SCRIPT_DIR/../../opensource/fbcode_builder/getdeps.py"
)
for getdeps in "${GETDEPS_PATHS[@]}"; do
    if [[ -x "$getdeps" ]]; then
        exec "$getdeps" build eden "$@"
    fi
done
echo "Could not find getdeps.py" >&2
exit 1
