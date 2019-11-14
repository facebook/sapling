# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "cat" << r"""
[extensions]
amend=
smartlog=
[experimental]
evolution = createmarkers
""" >> "$HGRCPATH"

# Test that changesets with visible precursors are rendered as x's
sh % "hg init repo"
sh % "cd repo"
sh % "hg debugbuilddag +4"
sh % "hg book -r 3 test"
sh % "hg up 1" == "0 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "hg amend -m amended --no-rebase" == r"""
    hint[amend-restack]: descendants of 66f7d451a68b are left behind - use 'hg restack' to rebase them
    hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints"""
sh % "hg smartlog -T '{rev} {bookmarks}'" == r"""
    @  4
    |
    | o  3 test
    | |
    | o  2
    | |
    | x  1
    |/
    o  0"""
sh % "hg unamend"
sh % "hg up 2" == "0 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "hg phase -r . --public"
sh % "hg smartlog -T '{rev} {bookmarks}'" == r"""
    o  3 test
    |
    @  2
    |
    ~"""
