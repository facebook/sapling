# coding=utf-8
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


# Test bookmark -D
sh % "hg init book-D"
sh % "cd book-D"
(
    sh % "cat"
    << r"""
[extensions]
amend=
tweakdefaults=
[experimental]
evolution=all
"""
    >> ".hg/hgrc"
)
sh % "hg debugbuilddag '+4*2*2*2'"
sh % "hg bookmark -i -r 1 master"
sh % "hg bookmark -i -r 5 feature1"
sh % "hg bookmark -i -r 6 feature2"
sh % "hg log -G -T '{rev} {bookmarks}' -r 'all()'" == r"""
    o  6 feature2
    │
    │ o  5 feature1
    │ │
    o │  4
    │ │
    │ o  3
    ├─╯
    o  2
    │
    o  1 master
    │
    o  0"""
sh % "hg bookmark -D feature1" == r"""
    hiding commit 2dc09a01254d "r3"
    hiding commit 191de46dc8b9 "r5"
    2 changesets hidden
    removing bookmark 'feature1' (was at: 191de46dc8b9)
    1 bookmark removed"""
sh % "hg log -G -T '{rev} {bookmarks}' -r 'all()' --hidden" == r"""
    o  6 feature2
    │
    │ o  5
    │ │
    o │  4
    │ │
    │ o  3
    ├─╯
    o  2
    │
    o  1 master
    │
    o  0"""
