# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""back up draft commits in the cloud"""

from __future__ import absolute_import


def extsetup(ui):
    ui.debug(
        "not loading infinitepushbackup - this extension has been merged into the commitcloud extension\n"
    )
