#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from . import ui


# TODO: Implement a custom WindowsOutput class that provides color support in
# Windows console windows.
WindowsOutput = ui.PlainOutput
