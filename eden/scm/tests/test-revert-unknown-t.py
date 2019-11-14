# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "hg init"
sh % "touch unknown"

sh % "touch a"
sh % "hg add a"
sh % "hg ci -m 1"

sh % "touch b"
sh % "hg add b"
sh % "hg ci -m 2"

# Should show unknown

sh % "hg status" == "? unknown"
sh % "hg revert -r 0 --all" == "removing b"

# Should show unknown and b removed

sh % "hg status" == r"""
    R b
    ? unknown"""

# Should show a and unknown

sh % "ls" == r"""
    a
    unknown"""
