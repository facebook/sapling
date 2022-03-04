# coding=utf-8
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


(
    sh % "cat"
    << r"""
[extensions]
absorb=
"""
    >> "$HGRCPATH"
)

sh % "hg init repo"
sh % "cd repo"
(
    sh % "hg debugdrawdag"
    << r"""
C
|
B
|
A
"""
)

sh % "hg debugmakepublic -r A"

sh % "hg update C -q"
sh % "printf B1" > "B"

sh % "hg absorb -aq"

sh % "hg log -G -T '{desc} {phase}'" == r"""
    @  C draft
    │
    o  B draft
    │
    o  A public"""
