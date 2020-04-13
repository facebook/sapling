# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "cat" << r"""
[extensions]
dirsync=
""" >> "$HGRCPATH"

sh % "hg init repo"
sh % "cd repo"
sh % "cat" << r"""
[ui]
verbose=true
[dirsync]
sync1.1=dir1/
sync1.2=dir2/subdir/
""" >> ".hg/hgrc"

# Add multiple files
sh % "mkdir dir1"
sh % "echo a" > "dir1/a"
sh % "echo b" > "dir1/b"
sh % "hg commit -Am 'Adding a and b'" == r"""
    adding dir1/a
    adding dir1/b
    mirrored adding 'dir1/a' to 'dir2/subdir/a'
    mirrored adding 'dir1/b' to 'dir2/subdir/b'
    committing files:
    dir1/a
    dir1/b
    dir2/subdir/a
    dir2/subdir/b
    committing manifest
    committing changelog
    committed changeset 0:32bc2a06fd26"""
sh % "hg diff --git -r null -r ." == r"""
    diff --git a/dir1/a b/dir1/a
    new file mode 100644
    --- /dev/null
    +++ b/dir1/a
    @@ -0,0 +1,1 @@
    +a
    diff --git a/dir1/b b/dir1/b
    new file mode 100644
    --- /dev/null
    +++ b/dir1/b
    @@ -0,0 +1,1 @@
    +b
    diff --git a/dir2/subdir/a b/dir2/subdir/a
    new file mode 100644
    --- /dev/null
    +++ b/dir2/subdir/a
    @@ -0,0 +1,1 @@
    +a
    diff --git a/dir2/subdir/b b/dir2/subdir/b
    new file mode 100644
    --- /dev/null
    +++ b/dir2/subdir/b
    @@ -0,0 +1,1 @@
    +b"""

# Include only changes to particular file
sh % "echo a" >> "dir1/a"
sh % "echo b" >> "dir1/b"
sh % "hg commit --amend -I dir1/a" == r"""
    mirrored changes in 'dir1/a' to 'dir2/subdir/a'
    amending changeset 32bc2a06fd26
    committing files:
    dir1/a
    dir1/b
    dir2/subdir/a
    dir2/subdir/b
    committing manifest
    committing changelog
    committed changeset 1:e9cce3b53a7c"""

sh % "hg diff --git -r null -r ." == r"""
    diff --git a/dir1/a b/dir1/a
    new file mode 100644
    --- /dev/null
    +++ b/dir1/a
    @@ -0,0 +1,2 @@
    +a
    +a
    diff --git a/dir1/b b/dir1/b
    new file mode 100644
    --- /dev/null
    +++ b/dir1/b
    @@ -0,0 +1,1 @@
    +b
    diff --git a/dir2/subdir/a b/dir2/subdir/a
    new file mode 100644
    --- /dev/null
    +++ b/dir2/subdir/a
    @@ -0,0 +1,2 @@
    +a
    +a
    diff --git a/dir2/subdir/b b/dir2/subdir/b
    new file mode 100644
    --- /dev/null
    +++ b/dir2/subdir/b
    @@ -0,0 +1,1 @@
    +b"""

sh % "echo a" >> "dir1/a"
sh % "hg commit --amend dir1/b" == r"""
    mirrored changes in 'dir1/b' to 'dir2/subdir/b'
    amending changeset e9cce3b53a7c
    committing files:
    dir1/a
    dir1/b
    dir2/subdir/a
    dir2/subdir/b
    committing manifest
    committing changelog
    committed changeset 2:a70e8a6cacdd"""

sh % "hg diff --git -r null -r ." == r"""
    diff --git a/dir1/a b/dir1/a
    new file mode 100644
    --- /dev/null
    +++ b/dir1/a
    @@ -0,0 +1,2 @@
    +a
    +a
    diff --git a/dir1/b b/dir1/b
    new file mode 100644
    --- /dev/null
    +++ b/dir1/b
    @@ -0,0 +1,2 @@
    +b
    +b
    diff --git a/dir2/subdir/a b/dir2/subdir/a
    new file mode 100644
    --- /dev/null
    +++ b/dir2/subdir/a
    @@ -0,0 +1,2 @@
    +a
    +a
    diff --git a/dir2/subdir/b b/dir2/subdir/b
    new file mode 100644
    --- /dev/null
    +++ b/dir2/subdir/b
    @@ -0,0 +1,2 @@
    +b
    +b"""

# Exclude changes to particular file
sh % "echo b" >> "dir1/b"
sh % "hg commit --amend -X dir1/a" == r"""
    mirrored changes in 'dir1/b' to 'dir2/subdir/b'
    amending changeset a70e8a6cacdd
    committing files:
    dir1/a
    dir1/b
    dir2/subdir/a
    dir2/subdir/b
    committing manifest
    committing changelog
    committed changeset 3:4af805a433df"""
sh % "hg diff --git -r null -r ." == r"""
    diff --git a/dir1/a b/dir1/a
    new file mode 100644
    --- /dev/null
    +++ b/dir1/a
    @@ -0,0 +1,2 @@
    +a
    +a
    diff --git a/dir1/b b/dir1/b
    new file mode 100644
    --- /dev/null
    +++ b/dir1/b
    @@ -0,0 +1,3 @@
    +b
    +b
    +b
    diff --git a/dir2/subdir/a b/dir2/subdir/a
    new file mode 100644
    --- /dev/null
    +++ b/dir2/subdir/a
    @@ -0,0 +1,2 @@
    +a
    +a
    diff --git a/dir2/subdir/b b/dir2/subdir/b
    new file mode 100644
    --- /dev/null
    +++ b/dir2/subdir/b
    @@ -0,0 +1,3 @@
    +b
    +b
    +b"""

# Check the addremove flag
sh % "echo c" > "dir1/c"
sh % "rm dir1/a"
sh % "hg commit --amend -A" == r"""
    removing dir1/a
    adding dir1/c
    mirrored adding 'dir1/c' to 'dir2/subdir/c'
    mirrored remove of 'dir1/a' to 'dir2/subdir/a'
    amending changeset 4af805a433df
    committing files:
    dir1/b
    dir1/c
    dir2/subdir/b
    dir2/subdir/c
    committing manifest
    committing changelog
    committed changeset 4:55c6a18e7d57"""

sh % "hg diff --git -r null -r ." == r"""
    diff --git a/dir1/b b/dir1/b
    new file mode 100644
    --- /dev/null
    +++ b/dir1/b
    @@ -0,0 +1,3 @@
    +b
    +b
    +b
    diff --git a/dir1/c b/dir1/c
    new file mode 100644
    --- /dev/null
    +++ b/dir1/c
    @@ -0,0 +1,1 @@
    +c
    diff --git a/dir2/subdir/b b/dir2/subdir/b
    new file mode 100644
    --- /dev/null
    +++ b/dir2/subdir/b
    @@ -0,0 +1,3 @@
    +b
    +b
    +b
    diff --git a/dir2/subdir/c b/dir2/subdir/c
    new file mode 100644
    --- /dev/null
    +++ b/dir2/subdir/c
    @@ -0,0 +1,1 @@
    +c"""
