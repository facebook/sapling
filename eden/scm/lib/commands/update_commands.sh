#!/bin/sh
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# use 'python3 -m pip install cogapp' to install cog
python3 -m cogapp -r TARGETS src/commands.rs
