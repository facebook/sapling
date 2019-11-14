# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "hg init"
sh % "touch a"
sh % "hg add a"
sh % "hg ci -m a"

sh % "echo 123" > "b"
sh % "hg add b"
sh % "hg diff --nodates" == r"""
    diff -r 3903775176ed b
    --- /dev/null
    +++ b/b
    @@ -0,0 +1,1 @@
    +123"""

sh % "hg diff --nodates -r tip" == r"""
    diff -r 3903775176ed b
    --- /dev/null
    +++ b/b
    @@ -0,0 +1,1 @@
    +123"""

sh % "echo foo" > "a"
sh % "hg diff --nodates" == r"""
    diff -r 3903775176ed a
    --- a/a
    +++ b/a
    @@ -0,0 +1,1 @@
    +foo
    diff -r 3903775176ed b
    --- /dev/null
    +++ b/b
    @@ -0,0 +1,1 @@
    +123"""

sh % "hg diff -r ''" == r"""
    hg: parse error: empty query
    [255]"""
sh % "hg diff -r tip -r ''" == r"""
    hg: parse error: empty query
    [255]"""

# Remove a file that was added via merge. Since the file is not in parent 1,
# it should not be in the diff.

sh % "hg ci -m 'a=foo' a"
sh % "hg co -Cq null"
sh % "echo 123" > "b"
sh % "hg add b"
sh % "hg ci -m b"
sh % "hg merge 1" == r"""
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved
    (branch merge, don't forget to commit)"""
sh % "hg rm -f a"
sh % "hg diff --nodates"

# Rename a file that was added via merge. Since the rename source is not in
# parent 1, the diff should be relative to /dev/null

sh % "hg co -Cq 2"
sh % "hg merge 1" == r"""
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved
    (branch merge, don't forget to commit)"""
sh % "hg mv a a2"
sh % "hg diff --nodates" == r"""
    diff -r cf44b38435e5 a2
    --- /dev/null
    +++ b/a2
    @@ -0,0 +1,1 @@
    +foo"""
sh % "hg diff --nodates --git" == r"""
    diff --git a/a2 b/a2
    new file mode 100644
    --- /dev/null
    +++ b/a2
    @@ -0,0 +1,1 @@
    +foo"""
