# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


# This test file aims at test topological iteration and the various configuration it can has.

sh % "cat" << r"""
[ui]
logtemplate={rev}\n
""" >> "$HGRCPATH"

# On this simple example, all topological branch are displayed in turn until we
# can finally display 0. this implies skipping from 8 to 3 and coming back to 7
# later.

sh % "hg init test01"
sh % "cd test01"
sh % 'hg unbundle "$TESTDIR/bundles/remote.hg"' == r"""
    adding changesets
    adding manifests
    adding file changes
    added 9 changesets with 7 changes to 4 files"""

sh % "hg log -G" == r"""
    o  8
    |
    | o  7
    | |
    | o  6
    | |
    | o  5
    | |
    | o  4
    | |
    o |  3
    | |
    o |  2
    | |
    o |  1
    |/
    o  0"""

# (display all nodes)

sh % "hg log -G -r 'sort(all(), topo)'" == r"""
    o  8
    |
    o  3
    |
    o  2
    |
    o  1
    |
    | o  7
    | |
    | o  6
    | |
    | o  5
    | |
    | o  4
    |/
    o  0"""

# (display nodes filtered by log options)

sh % "hg log -G -r 'sort(all(), topo)' -k .3" == r"""
    o  8
    |
    o  3
    |
    ~
    o  7
    |
    o  6
    |
    ~"""

# (revset skipping nodes)

sh % "hg log -G --rev 'sort(not (2+6), topo)'" == r"""
    o  8
    |
    o  3
    :
    o  1
    |
    | o  7
    | :
    | o  5
    | |
    | o  4
    |/
    o  0"""

# (begin) from the other branch

sh % "hg log -G -r 'sort(all(), topo, topo.firstbranch=5)'" == r"""
    o  7
    |
    o  6
    |
    o  5
    |
    o  4
    |
    | o  8
    | |
    | o  3
    | |
    | o  2
    | |
    | o  1
    |/
    o  0"""
