#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

  $ cat >> $HGRCPATH << 'EOF'
  > [extensions]
  > dirsync=
  > EOF

  $ hg init repo
  $ cd repo
  $ cat >> .hg/hgrc << 'EOF'
  > [ui]
  > verbose=true
  > [dirsync]
  > sync1.1=dir1/
  > sync1.2=dir2/subdir/
  > EOF

# Add multiple files

  $ mkdir dir1
  $ echo a > dir1/a
  $ echo b > dir1/b
  $ hg commit -Am 'Adding a and b'
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
  committed * (glob)
  $ hg diff --git -r null -r .
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
  +b

# Include only changes to particular file

  $ echo a >> dir1/a
  $ echo b >> dir1/b
  $ hg commit --amend -I dir1/a
  amending changeset * (glob)
  mirrored adding 'dir1/a' to 'dir2/subdir/a'
  committing files:
  dir1/a
  dir1/b
  dir2/subdir/a
  dir2/subdir/b
  committing manifest
  committing changelog
  committed * (glob)

  $ hg diff --git -r null -r .
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
  +b

  $ echo a >> dir1/a
  $ hg commit --amend dir1/b
  amending changeset * (glob)
  mirrored adding 'dir1/b' to 'dir2/subdir/b'
  committing files:
  dir1/a
  dir1/b
  dir2/subdir/a
  dir2/subdir/b
  committing manifest
  committing changelog
  committed * (glob)

  $ hg diff --git -r null -r .
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
  +b

# Exclude changes to particular file

  $ echo b >> dir1/b
  $ hg commit --amend -X dir1/a
  amending changeset * (glob)
  mirrored adding 'dir1/b' to 'dir2/subdir/b'
  committing files:
  dir1/a
  dir1/b
  dir2/subdir/a
  dir2/subdir/b
  committing manifest
  committing changelog
  committed * (glob)
  $ hg diff --git -r null -r .
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
  +b

# Check the addremove flag

  $ echo c > dir1/c
  $ rm dir1/a
  $ hg commit --amend -A
  amending changeset * (glob)
  removing dir1/a
  adding dir1/c
  mirrored adding 'dir1/c' to 'dir2/subdir/c'
  mirrored remove of 'dir1/a' to 'dir2/subdir/a'
  committing files:
  dir1/b
  dir1/c
  dir2/subdir/b
  dir2/subdir/c
  committing manifest
  committing changelog
  committed * (glob)

  $ hg diff --git -r null -r .
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
  +c
