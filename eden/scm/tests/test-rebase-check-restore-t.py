# coding=utf-8

# Copyright (c) Meta Platforms, Inc. and affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "setconfig experimental.allowfilepeer=True"
sh % "setconfig 'extensions.treemanifest=!'"
# TODO: Make this test compatibile with obsstore enabled.
sh % "setconfig 'experimental.evolution='"
(
    sh % "cat"
    << r"""
[extensions]
rebase=

[phases]
publish=False
"""
    >> "$HGRCPATH"
)


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

sh % "hg up -q -C min(_all())"

sh % "echo D" >> "A"
sh % "hg ci -m D"

sh % "echo E" > "E"
sh % "hg add E"
sh % "hg ci -m E"

sh % "hg up -q -C min(_all())"

sh % "echo F" >> "A"
sh % "hg ci -m F"

sh % "cd .."


# Rebasing B onto E - check keep: and phases

sh % "hg clone -q -u . a a1"
sh % "cd a1"

sh % "tglogp" == r"""
    @  3225f3ea730a draft 'F'
    │
    │ o  ae36e8e3dfd7 draft 'E'
    │ │
    │ o  46b37eabc604 draft 'D'
    ├─╯
    │ o  965c486023db draft 'C'
    │ │
    │ o  27547f69f254 draft 'B'
    ├─╯
    o  4a2df7238c3b draft 'A'"""
sh % "hg rebase -s desc(B) -d desc(E) --keep" == r"""
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
    o  d2d25e26288e draft 'C'
    │
    o  45396c49d53b draft 'B'
    │
    │ @  3225f3ea730a draft 'F'
    │ │
    o │  ae36e8e3dfd7 draft 'E'
    │ │
    o │  46b37eabc604 draft 'D'
    ├─╯
    │ o  965c486023db draft 'C'
    │ │
    │ o  27547f69f254 draft 'B'
    ├─╯
    o  4a2df7238c3b draft 'A'"""
sh % "cd .."


# Rebase F onto E:

sh % "hg clone -q -u . a a2"
sh % "cd a2"

sh % "tglogp" == r"""
    @  3225f3ea730a draft 'F'
    │
    │ o  ae36e8e3dfd7 draft 'E'
    │ │
    │ o  46b37eabc604 draft 'D'
    ├─╯
    │ o  965c486023db draft 'C'
    │ │
    │ o  27547f69f254 draft 'B'
    ├─╯
    o  4a2df7238c3b draft 'A'"""
sh % "hg rebase -s desc(F) -d desc(E)" == r"""
    rebasing 3225f3ea730a "F"
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
sh % "hg rebase --continue" == 'rebasing 3225f3ea730a "F"'

sh % "tglogp" == r"""
    @  530bc6058bd0 draft 'F'
    │
    o  ae36e8e3dfd7 draft 'E'
    │
    o  46b37eabc604 draft 'D'
    │
    │ o  965c486023db draft 'C'
    │ │
    │ o  27547f69f254 draft 'B'
    ├─╯
    o  4a2df7238c3b draft 'A'"""

sh % "cd .."
