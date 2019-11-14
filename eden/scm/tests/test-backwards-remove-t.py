# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "hg init"
sh % "echo This is file a1" > "a"
sh % "hg add a"
sh % "hg commit -m 'commit #0'"
sh % "ls" == "a"
sh % "echo This is file b1" > "b"
sh % "hg add b"
sh % "hg commit -m 'commit #1'"
sh % "hg co 0" == "0 files updated, 0 files merged, 1 files removed, 0 files unresolved"

# B should disappear

sh % "ls" == "a"
