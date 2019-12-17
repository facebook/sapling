# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


# Verify ancestry data is readable by mergedrivers by looking at mergestate:

sh % "newrepo"
sh % "enable rebase"
sh % "setconfig 'experimental.evolution='"
sh % "setconfig 'rebase.singletransaction=True'"
sh % "setconfig 'rebase.experimental.inmemory=True'"

sh % "mkdir driver"
sh % "cat" << r"""
def preprocess(ui, repo, hooktype, mergestate, wctx, labels=None):
    unresolved_files = list(mergestate.unresolved())
    ui.warn("ancestor nodes = %s\n" % [ctx.hex() for ctx in mergestate.ancestorctxs])
    ui.warn("ancestor revs = %s\n" % [ctx.rev() for ctx in mergestate.ancestorctxs])
    mergestate.commit()
def conclude(ui, repo, hooktype, mergestate, wctx, labels=None):
    pass
""" > "driver/__init__.py"

sh % "setconfig 'experimental.mergedriver=python:driver/'"
sh % "hg commit -Aqm driver"
sh % "hg debugdrawdag" << r"""
E    # E/file = 1\n2\n3\n4\n5
|
D
|
C F b  # F/file = 0\n1\n2\n3\n4
|/
B
|
A   # A/file = 1\n2\n3\n4
"""
sh % "hg rebase -s A -d 0" == r"""
    rebasing 19c6d3b0d8fb "A" (A)
    rebasing 5a83467e1fc3 "B" (B)
    rebasing 09810f6b52c0 "F" (F)
    rebasing 3ff755c5931b "C" (C)
    rebasing dc7f2675f9ab "D" (D)
    rebasing 5eb863826611 "E" (E)
    saved backup bundle to $TESTTMP/repo1/.hg/strip-backup/19c6d3b0d8fb-a2cf1ad8-rebase.hg"""
sh % "showgraph" == r"""
    o  7 e71547946f82 E
    |
    o  6 264c021e8fc6 D
    |
    o  5 34e41e21cd9d C
    |
    | o  4 aa431a9572c1 F
    |/
    o  3 01ba3ad89eb7 B
    |
    o  2 622e2d864a27 A
    |
    | o  1 520a9f665f6e b
    |
    @  0 9309aa3b805a driver"""
sh % "hg rebase -r aa431a9572c1 -d e71547946f82" == r"""
    rebasing aa431a9572c1 "F" (F)
    ancestor nodes = ['01ba3ad89eb70070d81f052c0c40a3877c2ba5d8']
    ancestor revs = [3]
    merging file
    saved backup bundle to $TESTTMP/repo1/.hg/strip-backup/aa431a9572c1-13824be1-rebase.hg"""
