# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


# Test bookmark -D
sh % "hg init book-D"
sh % "cd book-D"
sh % "cat" << r"""
[extensions]
amend=
tweakdefaults=
[experimental]
evolution=all
""" >> ".hg/hgrc"
sh % "hg debugbuilddag '+4*2*2*2'"
sh % "hg bookmark -i -r 1 master"
sh % "hg bookmark -i -r 5 feature1"
sh % "hg bookmark -i -r 6 feature2"
sh % "hg log -G -T '{rev} {bookmarks}' -r 'all()'" == r"""
    o  6 feature2
    |
    | o  5 feature1
    | |
    o |  4
    | |
    | o  3
    |/
    o  2
    |
    o  1 master
    |
    o  0"""
sh % "hg bookmark -D feature1" == r"""
    bookmark 'feature1' deleted
    2 changesets pruned
    hint[strip-hide]: 'hg strip' may be deprecated in the future - use 'hg hide' instead
    hint[hint-ack]: use 'hg hint --ack strip-hide' to silence these hints"""
sh % "hg log -G -T '{rev} {bookmarks}' -r 'all()' --hidden" == r"""
    o  6 feature2
    |
    | x  5
    | |
    o |  4
    | |
    | x  3
    |/
    o  2
    |
    o  1 master
    |
    o  0"""
