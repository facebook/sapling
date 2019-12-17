# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


# TODO: Make this test compatibile with obsstore enabled.
sh % "setconfig 'experimental.evolution='"

# Set up extensions (order is important here, we must test tweakdefaults loading last)
sh % "cat" << r"""
[extensions]
rebase=
remotenames=
tweakdefaults=
""" >> "$HGRCPATH"

# Run test
sh % "hg init repo"
sh % "cd repo"
sh % "touch a"
sh % "hg commit -Aqm a"
sh % "touch b"
sh % "hg commit -Aqm b"
sh % "hg bookmark AB"
sh % "hg up '.^'" == r"""
    0 files updated, 0 files merged, 1 files removed, 0 files unresolved
    (leaving bookmark AB)"""
sh % "touch c"
sh % "hg commit -Aqm c"
sh % "hg bookmark C -t AB"
sh % "hg rebase" == r"""
    rebasing d5e255ef74f8 "c" (C)
    saved backup bundle to $TESTTMP/repo/.hg/strip-backup/d5e255ef74f8-7d2cc323-rebase.hg"""
