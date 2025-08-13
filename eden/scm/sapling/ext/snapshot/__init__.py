# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""stores snapshots of uncommitted changes

Configs::

    [snapshot]
    # Whether to allow creating empty snapshots (default: True)
    # When set to False, snapshot creation will be skipped if there are no uncommitted changes
    allowempty = True

    # Maximum size for untracked files to be included in snapshots (default: 1GB)
    # Files larger than this size will be excluded from snapshots with a warning
    # Accepts human-readable values like "1GB", "500MB", "2048KB", or raw bytes
    maxuntrackedsize = 1GB
"""

from . import commands

cmdtable = commands.cmdtable
