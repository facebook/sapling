# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "setconfig 'extensions.treemanifest=!'"
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

sh % "echo A" > "A"
sh % "hg add A"
sh % "hg ci -m A"

sh % "echo B" > "B"
sh % "hg add B"
sh % "hg ci -m B"

sh % "echo C" >> "A"
sh % "hg ci -m C"

sh % "hg up -q -C 0"

sh % "echo D" >> "A"
sh % "hg ci -m D"

sh % "echo E" > "E"
sh % "hg add E"
sh % "hg ci -m E"

sh % "hg up -q -C 0"

sh % "echo F" >> "A"
sh % "hg ci -m F"

sh % "cd .."


# Rebasing B onto E - check keep: and phases

sh % "hg clone -q -u . a a1"
sh % "cd a1"
sh % "hg phase --force --secret 2"

sh % "tglogp" == r"""
    @  5: 3225f3ea730a draft 'F'
    |
    | o  4: ae36e8e3dfd7 draft 'E'
    | |
    | o  3: 46b37eabc604 draft 'D'
    |/
    | o  2: 965c486023db secret 'C'
    | |
    | o  1: 27547f69f254 draft 'B'
    |/
    o  0: 4a2df7238c3b draft 'A'"""
sh % "hg rebase -s 1 -d 4 --keep" == r"""
    rebasing 27547f69f254 "B"
    rebasing 965c486023db "C"
    merging A
    warning: 1 conflicts while merging A! (edit, then use 'hg resolve --mark')
    unresolved conflicts (see hg resolve, then hg rebase --continue)
    [1]"""

# Solve the conflict and go on:

sh % "echo 'conflict solved'" > "A"
sh % "rm A.orig"
sh % "hg resolve -m A" == r"""
    (no more unresolved files)
    continue: hg rebase --continue"""
sh % "hg rebase --continue" == r'''
    already rebased 27547f69f254 "B" as 45396c49d53b
    rebasing 965c486023db "C"'''

sh % "tglogp" == r"""
    o  7: d2d25e26288e secret 'C'
    |
    o  6: 45396c49d53b draft 'B'
    |
    | @  5: 3225f3ea730a draft 'F'
    | |
    o |  4: ae36e8e3dfd7 draft 'E'
    | |
    o |  3: 46b37eabc604 draft 'D'
    |/
    | o  2: 965c486023db secret 'C'
    | |
    | o  1: 27547f69f254 draft 'B'
    |/
    o  0: 4a2df7238c3b draft 'A'"""
sh % "cd .."


# Rebase F onto E:

sh % "hg clone -q -u . a a2"
sh % "cd a2"
sh % "hg phase --force --secret 2"

sh % "tglogp" == r"""
    @  5: 3225f3ea730a draft 'F'
    |
    | o  4: ae36e8e3dfd7 draft 'E'
    | |
    | o  3: 46b37eabc604 draft 'D'
    |/
    | o  2: 965c486023db secret 'C'
    | |
    | o  1: 27547f69f254 draft 'B'
    |/
    o  0: 4a2df7238c3b draft 'A'"""
sh % "hg rebase -s 5 -d 4" == r"""
    rebasing 3225f3ea730a "F" (tip)
    merging A
    warning: 1 conflicts while merging A! (edit, then use 'hg resolve --mark')
    unresolved conflicts (see hg resolve, then hg rebase --continue)
    [1]"""

# Solve the conflict and go on:

sh % "echo 'conflict solved'" > "A"
sh % "rm A.orig"
sh % "hg resolve -m A" == r"""
    (no more unresolved files)
    continue: hg rebase --continue"""
sh % "hg rebase --continue" == r"""
    rebasing 3225f3ea730a "F" (tip)
    saved backup bundle to $TESTTMP/a2/.hg/strip-backup/3225f3ea730a-289ce185-rebase.hg"""

sh % "tglogp" == r"""
    @  5: 530bc6058bd0 draft 'F'
    |
    o  4: ae36e8e3dfd7 draft 'E'
    |
    o  3: 46b37eabc604 draft 'D'
    |
    | o  2: 965c486023db secret 'C'
    | |
    | o  1: 27547f69f254 draft 'B'
    |/
    o  0: 4a2df7238c3b draft 'A'"""

sh % "cd .."
