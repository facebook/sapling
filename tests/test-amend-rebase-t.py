# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


# Set up test environment.
sh % "cat" << r"""
[extensions]
amend=
rebase=
[experimental]
evolution = createmarkers, allowunstable
""" >> "$HGRCPATH"

# Test that rebased commits that would cause instability are inhibited.
sh % "hg init repo"
sh % "cd repo"
sh % "hg debugbuilddag -m '+3 *3'"
sh % "showgraph" == r"""
    o  3 e5d56d7a7894 r3
    |
    | o  2 c175bafe34cb r2
    | |
    | o  1 22094967a90d r1
    |/
    o  0 1ad88bca4140 r0"""
sh % "hg rebase -r 1 -d 3" == r"""
    rebasing 1:* "r1" (glob)
    merging mf"""
sh % "showgraph" == r"""
    o  4 309a29d7f33b r1
    |
    o  3 e5d56d7a7894 r3
    |
    | o  2 c175bafe34cb r2
    | |
    | x  1 22094967a90d r1
    |/
    o  0 1ad88bca4140 r0"""
