# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "setconfig 'extensions.treemanifest=!'"
sh % ". helpers-usechg.sh"

# Test for changeset ba7c74081861
# (update dirstate correctly for non-branchmerge updates)
sh % "hg init a"
sh % "cd a"
sh % "echo a" > "a"
sh % "hg add a"
sh % "hg commit -m a"
sh % "cd .."
sh % "hg clone a b" == r"""
    updating to branch default
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "cd a"
sh % "hg mv a b"
sh % "hg commit -m move"
sh % "echo b" >> "b"
sh % "hg commit -m b"
sh % "cd ../b"
sh % "hg pull ../a" == r"""
    pulling from ../a
    searching for changes
    adding changesets
    adding manifests
    adding file changes
    added 2 changesets with 2 changes to 1 files
    new changesets e3c9b40284e1:772b37f1ca37"""
sh % "hg update" == "1 files updated, 0 files merged, 1 files removed, 0 files unresolved"

sh % "cd .."
