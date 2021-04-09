# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


# Empty update fails with a helpful error:

sh % "setconfig 'ui.disallowemptyupdate=True'"
sh % "newrepo"
sh % "hg debugdrawdag" << r"""
B
|
A
"""
sh % "hg up -q 0"
sh % "hg up" == r"""
    abort: You must specify a destination to update to, for example "hg update master".
    (If you're trying to move a bookmark forward, try "hg rebase -d <destination>".)
    [255]"""

# up -r works as intended:
sh % "hg up -q -r 1"
sh % "hg log -r . -T '{rev}\\n'" == "1"
sh % "hg up -q 1"
sh % "hg log -r . -T '{rev}\\n'" == "1"
