# coding=utf-8

# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


# Set up test environment.
(
    sh % "cat"
    << r"""
[extensions]
amend=
rebase=
[experimental]
evolution = obsolete
[mutation]
enabled=true
record=false
[visibility]
enabled=true
"""
    >> "$HGRCPATH"
)

# Test that rebases that cause an orphan commit are not a problem.
sh % "hg init repo"
sh % "cd repo"
sh % "hg debugbuilddag -m '+3 *3'"
sh % "showgraph" == r"""
    o  e5d56d7a7894 r3
    │
    │ o  c175bafe34cb r2
    │ │
    │ o  22094967a90d r1
    ├─╯
    o  1ad88bca4140 r0"""
sh % "hg rebase -r 1 -d 3" == r"""
    rebasing 22094967a90d "r1"
    merging mf"""
sh % "showgraph" == r"""
    o  309a29d7f33b r1
    │
    o  e5d56d7a7894 r3
    │
    │ o  c175bafe34cb r2
    │ │
    │ x  22094967a90d r1
    ├─╯
    o  1ad88bca4140 r0"""
