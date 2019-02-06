# disablesymlinks.py
#
# Copyright 2019 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""disables symlink support when enabled"""

from __future__ import absolute_import

from edenscm.mercurial import posix, util


def checklink(path):
    return False


def uisetup(ui):
    posix.checklink = checklink
    util.checklink = checklink
