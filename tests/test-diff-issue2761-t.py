# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


# Test issue2761

sh % "hg init"

sh % "touch to-be-deleted"
sh % "hg add" == "adding to-be-deleted"
sh % "hg ci -m first"
sh % "echo a" > "to-be-deleted"
sh % "hg ci -m second"
sh % "rm to-be-deleted"
sh % "hg diff -r 0"

# Same issue, different code path

sh % "hg up -C" == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "touch does-not-exist-in-1"
sh % "hg add" == "adding does-not-exist-in-1"
sh % "hg ci -m third"
sh % "rm does-not-exist-in-1"
sh % "hg diff -r 1"
