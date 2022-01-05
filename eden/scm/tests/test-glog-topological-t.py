# coding=utf-8

# coding=utf-8

# Copyright (c) Meta Platforms, Inc. and affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


# This test file aims at test topological iteration and the various configuration it can has.

(
    sh % "cat"
    << r"""
[ui]
logtemplate={rev}\n
allowemptycommit=True
"""
    >> "$HGRCPATH"
)

# On this simple example, all topological branch are displayed in turn until we
# can finally display 0. this implies skipping from 8 to 3 and coming back to 7
# later.

sh % "hg init test01"
sh % "cd test01"
sh % "hg commit -qm 0"
sh % "hg commit -qm 1"
sh % "hg commit -qm 2"
sh % "hg commit -qm 3"
sh % "hg up -q 0"
sh % "hg commit -qm 4"
sh % "hg commit -qm 5"
sh % "hg commit -qm 6"
sh % "hg commit -qm 7"
sh % "hg up -q 3"
sh % "hg commit -qm 8"
sh % "hg up -q null"

sh % "hg log -G" == r"""
    o  8
    │
    │ o  7
    │ │
    │ o  6
    │ │
    │ o  5
    │ │
    │ o  4
    │ │
    o │  3
    │ │
    o │  2
    │ │
    o │  1
    ├─╯
    o  0"""

# (display all nodes)

sh % "hg log -G -r 'sort(all(), topo)'" == r"""
    o  8
    │
    o  3
    │
    o  2
    │
    o  1
    │
    │ o  7
    │ │
    │ o  6
    │ │
    │ o  5
    │ │
    │ o  4
    ├─╯
    o  0"""

# (revset skipping nodes)

sh % "hg log -G --rev 'sort(not (2+6), topo)'" == r"""
    o  8
    │
    o  3
    ╷
    o  1
    │
    │ o  7
    │ ╷
    │ o  5
    │ │
    │ o  4
    ├─╯
    o  0"""

# (begin) from the other branch

sh % "hg log -G -r 'sort(all(), topo, topo.firstbranch=5)'" == r"""
    o  7
    │
    o  6
    │
    o  5
    │
    o  4
    │
    │ o  8
    │ │
    │ o  3
    │ │
    │ o  2
    │ │
    │ o  1
    ├─╯
    o  0"""
