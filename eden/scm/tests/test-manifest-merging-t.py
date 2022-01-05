# Copyright (c) Meta Platforms, Inc. and affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401

sh % "configure modernclient"

sh % "newclientrepo base"

sh % "echo alpha" > "alpha"
sh % "hg ci -A -m 'add alpha'" == "adding alpha"
sh % "hg push -q --to book --create"
sh % "cd .."

sh % "newclientrepo work test:base_server book"

sh % "echo beta" > "beta"
sh % "hg ci -A -m 'add beta'" == "adding beta"
sh % "cd .."

sh % "cd base"
sh % "echo gamma" > "gamma"
sh % "hg ci -A -m 'add gamma'" == "adding gamma"
sh % "hg push -q --to book"
sh % "cd .."

sh % "cd work"
sh % "hg pull -q"
sh % "hg merge" == r"""
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved
    (branch merge, don't forget to commit)"""

# Update --clean to revision 1 to simulate a failed merge:

sh % "rm alpha beta gamma"
sh % "hg update --clean 'desc(beta)'" == "2 files updated, 0 files merged, 0 files removed, 0 files unresolved"

sh % "cd .."
