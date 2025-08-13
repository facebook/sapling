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
"""

from . import commands

cmdtable = commands.cmdtable
