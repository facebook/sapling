#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# You might need to set https proxy for Meta development servers
wget -O /tmp/pip.pyz https://bootstrap.pypa.io/pip/pip.pyz
sl debugpython -- /tmp/pip.pyz install --user IPython
