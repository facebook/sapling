#!/bin/sh
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

set -e

buck build //eden/scm/lib/xdiff:diff
REPO_ROOT=$(hg root)
XDIFF="$REPO_ROOT/fbcode/$(buck targets //eden/scm/lib/xdiff:diff --show-output | cut -d' ' -f2)"
export XDIFF
buck run fbsource//third-party/cram:cram -- "$@"
