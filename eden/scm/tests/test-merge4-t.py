# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % ". helpers-usechg.sh"

sh % "hg init"
sh % "echo This is file a1" > "a"
sh % "hg add a"
sh % "hg commit -m 'commit #0'"
sh % "echo This is file b1" > "b"
sh % "hg add b"
sh % "hg commit -m 'commit #1'"
sh % "hg update 0" == "0 files updated, 0 files merged, 1 files removed, 0 files unresolved"
sh % "echo This is file c1" > "c"
sh % "hg add c"
sh % "hg commit -m 'commit #2'"
sh % "hg merge 1" == r"""
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved
    (branch merge, don't forget to commit)"""
sh % "rm b"
sh % "echo This is file c22" > "c"

# Test hg behaves when committing with a missing file added by a merge

sh % "hg commit -m 'commit #3'" == r"""
    abort: cannot commit merge with missing files
    [255]"""
