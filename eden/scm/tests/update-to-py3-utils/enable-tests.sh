#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

set -e

./run-tests.py --with-hg=../hg3 --json || true
./update-to-py3-utils/retry-skipped.py
./run-tests.py --with-hg=../hg3 --json || true
./update-to-py3-utils/revert-failed.py
arc f
