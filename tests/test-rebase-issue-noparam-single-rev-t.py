# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


# TODO: Make this test compatibile with obsstore enabled.
sh % "setconfig 'experimental.evolution='"
sh % "cat" << r"""
[extensions]
rebase=

[phases]
publish=False
""" >> "$HGRCPATH"


sh % "hg init a"
sh % "cd a"

sh % "echo c1" > "c1"
sh % "hg ci -Am c1" == "adding c1"

sh % "echo c2" > "c2"
sh % "hg ci -Am c2" == "adding c2"

sh % "echo l1" > "l1"
sh % "hg ci -Am l1" == "adding l1"

sh % "hg up -q -C 1"

sh % "echo r1" > "r1"
sh % "hg ci -Am r1" == "adding r1"

sh % "echo r2" > "r2"
sh % "hg ci -Am r2" == "adding r2"

sh % "tglog" == r"""
    @  4: 225af64d03e6 'r2'
    |
    o  3: 8d0a8c99b309 'r1'
    |
    | o  2: 87c180a611f2 'l1'
    |/
    o  1: 56daeba07f4b 'c2'
    |
    o  0: e8faad3d03ff 'c1'"""
# Rebase with no arguments - single revision in source branch:

sh % "hg up -q -C 2"

sh % "hg rebase" == r"""
    rebasing 2:87c180a611f2 "l1"
    saved backup bundle to $TESTTMP/a/.hg/strip-backup/87c180a611f2-a5be192d-rebase.hg"""

sh % "tglog" == r"""
    @  4: b1152cc99655 'l1'
    |
    o  3: 225af64d03e6 'r2'
    |
    o  2: 8d0a8c99b309 'r1'
    |
    o  1: 56daeba07f4b 'c2'
    |
    o  0: e8faad3d03ff 'c1'"""
sh % "cd .."


sh % "hg init b"
sh % "cd b"

sh % "echo c1" > "c1"
sh % "hg ci -Am c1" == "adding c1"

sh % "echo c2" > "c2"
sh % "hg ci -Am c2" == "adding c2"

sh % "echo l1" > "l1"
sh % "hg ci -Am l1" == "adding l1"

sh % "echo l2" > "l2"
sh % "hg ci -Am l2" == "adding l2"

sh % "hg up -q -C 1"

sh % "echo r1" > "r1"
sh % "hg ci -Am r1" == "adding r1"

sh % "tglog" == r"""
    @  4: 8d0a8c99b309 'r1'
    |
    | o  3: 1ac923b736ef 'l2'
    | |
    | o  2: 87c180a611f2 'l1'
    |/
    o  1: 56daeba07f4b 'c2'
    |
    o  0: e8faad3d03ff 'c1'"""
# Rebase with no arguments - single revision in target branch:

sh % "hg up -q -C 3"

sh % "hg rebase" == r"""
    rebasing 2:87c180a611f2 "l1"
    rebasing 3:1ac923b736ef "l2"
    saved backup bundle to $TESTTMP/b/.hg/strip-backup/87c180a611f2-b980535c-rebase.hg"""

sh % "tglog" == r"""
    @  4: 023181307ed0 'l2'
    |
    o  3: 913ab52b43b4 'l1'
    |
    o  2: 8d0a8c99b309 'r1'
    |
    o  1: 56daeba07f4b 'c2'
    |
    o  0: e8faad3d03ff 'c1'"""

sh % "cd .."
