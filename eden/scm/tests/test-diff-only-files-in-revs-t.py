# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "hg init repo"
sh % "cd repo"

sh % "echo file1version1" > "file1"
sh % "echo file2version1" > "file2"
sh % "echo file3version1" > "file3"
sh % "hg commit -Aqm base"
sh % "echo file1version2" > "file1"
sh % "hg commit -qm 'file1 change1'"
sh % "echo file2version2" > "file2"
sh % "hg commit -qm 'unrelated change to file2'"
sh % "echo file1version3" > "file1"
sh % "hg commit -qm 'file1 change2'"
sh % "echo file3verison2" > "file3"

# Normal diff shows the unrelated change in the intervening commit.
sh % "hg diff -r 1 -r 3 --nodates" == r"""
    diff -r b586868a82b9 -r 362c080b3cff file1
    --- a/file1
    +++ b/file1
    @@ -1,1 +1,1 @@
    -file1version2
    +file1version3
    diff -r b586868a82b9 -r 362c080b3cff file2
    --- a/file2
    +++ b/file2
    @@ -1,1 +1,1 @@
    -file2version1
    +file2version2"""

# With --only-files-in-revs, that is excluded.
sh % "hg diff -r 1 -r 3 --nodates --only-files-in-revs" == r"""
    diff -r b586868a82b9 -r 362c080b3cff file1
    --- a/file1
    +++ b/file1
    @@ -1,1 +1,1 @@
    -file1version2
    +file1version3"""

# Similarly, with a single rev, only consider files modified in that rev and the working copy.
sh % "hg diff -r 1 --nodates --only-files-in-revs" == r"""
    diff -r b586868a82b9 file1
    --- a/file1
    +++ b/file1
    @@ -1,1 +1,1 @@
    -file1version2
    +file1version3
    diff -r b586868a82b9 file3
    --- a/file3
    +++ b/file3
    @@ -1,1 +1,1 @@
    -file3version1
    +file3verison2"""
