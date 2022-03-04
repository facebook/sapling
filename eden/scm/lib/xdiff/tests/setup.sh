# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# shellcheck shell=bash

if [[ -z "$XDIFF" ]]; then
    echo "Required XDIFF is not set." >&2
      exit 1
fi

# So bash expands our aliases.
[ "$TESTSHELL" = "/bin/bash" ] && shopt -s expand_aliases

alias xdiff="$XDIFF"
