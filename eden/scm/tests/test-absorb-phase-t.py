# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "cat" << r"""
[extensions]
absorb=
""" >> "$HGRCPATH"

sh % "hg init"
sh % "hg debugdrawdag" << r"""
C
|
B
|
A
"""

sh % "hg phase -r A --public -q"
sh % "hg phase -r C --secret --force -q"

sh % "hg update C -q"
sh % "printf B1" > "B"

sh % "hg absorb -aq"

sh % "hg log -G -T '{desc} {phase}'" == r"""
    @  C secret
    |
    o  B draft
    |
    o  A public"""
