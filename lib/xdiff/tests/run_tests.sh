#!/bin/sh
# (c) Facebook, Inc. and its affiliates. Confidential and proprietary.
# Copyright 2019 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
set -e

buck build //scm/hg/lib/xdiff:diff
REPO_ROOT=$(hg root)
XDIFF="$REPO_ROOT/fbcode/$(buck targets //scm/hg/lib/xdiff:diff --show-output | cut -d' ' -f2)"
export XDIFF
buck run fbsource//xplat/third-party/cram:cram -- "$@"
