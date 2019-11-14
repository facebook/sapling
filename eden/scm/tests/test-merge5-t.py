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
sh % "echo This is file b1" > "b"
sh % "hg add a b"
sh % "hg commit -m 'commit #0'"
sh % "echo This is file b22" > "b"
sh % "hg commit -m 'comment #1'"
sh % "hg update 0" == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "rm b"
sh % "hg commit -A -m 'comment #2'" == "removing b"
sh % "hg update 1" == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "rm b"
sh % "hg update -c 2" == r"""
    abort: uncommitted changes
    [255]"""
sh % "hg revert b"
sh % "hg update -c 2" == "0 files updated, 0 files merged, 1 files removed, 0 files unresolved"
sh % "mv a c"

# Should abort:

sh % "hg update 1" == r"""
    abort: uncommitted changes
    (commit or update --clean to discard changes)
    [255]"""
sh % "mv c a"

# Should succeed:

sh % "hg update 1" == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"
