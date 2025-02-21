#!/bin/sh
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# use 'python3 pip.pyz install --user cogapp' to install cog
${PYTHON:-python3} -m cogapp -r BUCK src/modules.rs
