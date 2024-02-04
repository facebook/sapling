# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""disables symlink support when enabled"""

from __future__ import absolute_import

from sapling import util


def checklink(path) -> bool:
    return False


def uisetup(ui) -> None:
    util.checklink = checklink
