# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "setconfig 'extensions.treemanifest=!'"
# Set up extension and repos

sh % "cat" << r"""
[extensions]
remotenames=
convert=
""" >> "$HGRCPATH"

sh % "hg init repo1"
sh % "cd repo1"
sh % "echo a" > "a"
sh % "hg add a"
sh % "hg commit -qm a"
sh % "hg boo bm2"
sh % "cd .."
sh % "hg clone repo1 repo2" == r"""
    updating to branch default
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""

# Test colors

sh % "hg -R repo2 bookmark --remote" == "   default/bm2               0:cb9a9f314b8b"
sh % "hg convert repo2 repo3" == r"""
    initializing destination repo3 repository
    scanning source...
    sorting...
    converting...
    0 a
    updating bookmarks"""
sh % "hg -R repo3 bookmark" == "   default/bm2               0:cb9a9f314b8b"
