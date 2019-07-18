# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "setconfig 'extensions.treemanifest=!'"
sh % ". helpers-usechg.sh"

# Test for changeset 9fe267f77f56ff127cf7e65dc15dd9de71ce8ceb
# (merge correctly when all the files in a directory are moved
# but then local changes are added in the same directory)

sh % "hg init a"
sh % "cd a"
sh % "mkdir -p testdir"
sh % "echo a" > "testdir/a"
sh % "hg add testdir/a"
sh % "hg commit -m a"
sh % "cd .."

sh % "hg clone a b" == r"""
    updating to branch default
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "cd a"
sh % "echo alpha" > "testdir/a"
sh % "hg commit -m remote-change"
sh % "cd .."

sh % "cd b"
sh % "mkdir testdir/subdir"
sh % "hg mv testdir/a testdir/subdir/a"
sh % "hg commit -m move"
sh % "mkdir newdir"
sh % "echo beta" > "newdir/beta"
sh % "hg add newdir/beta"
sh % "hg commit -m local-addition"
sh % "hg pull ../a" == r"""
    pulling from ../a
    searching for changes
    adding changesets
    adding manifests
    adding file changes
    added 1 changesets with 1 changes to 1 files (+1 heads)
    new changesets cc7000b01af9"""
sh % "hg up -C 2" == "0 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "hg merge" == r"""
    merging testdir/subdir/a and testdir/a to testdir/subdir/a
    0 files updated, 1 files merged, 0 files removed, 0 files unresolved
    (branch merge, don't forget to commit)"""
sh % "hg stat" == "M testdir/subdir/a"
sh % "hg diff --nodates" == r"""
    diff -r bc21c9773bfa testdir/subdir/a
    --- a/testdir/subdir/a
    +++ b/testdir/subdir/a
    @@ -1,1 +1,1 @@
    -a
    +alpha"""

sh % "cd .."
