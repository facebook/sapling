  $ extpath=$(dirname $TESTDIR)
  $ cp $extpath/dirsync.py $TESTTMP
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > dirsync=$TESTTMP/dirsync.py
  > EOF

  $ hg init repo
  $ cd repo
  $ cat >> .hg/hgrc <<EOF
  > [dirsync]
  > sync1.1=dir1/
  > sync1.2=dir2/subdir/
  > EOF

Test mirroring a simple add

  $ mkdir dir1/
  $ echo a > dir1/a
  $ hg add dir1/a
  $ hg commit --traceback -m "add in dir1"
  mirrored adding 'dir1/a' to 'dir2/subdir/a'
  $ hg diff --git -r null -r .
  diff --git a/dir1/a b/dir1/a
  new file mode 100644
  --- /dev/null
  +++ b/dir1/a
  @@ -0,0 +1,1 @@
  +a
  diff --git a/dir2/subdir/a b/dir2/subdir/a
  new file mode 100644
  --- /dev/null
  +++ b/dir2/subdir/a
  @@ -0,0 +1,1 @@
  +a

Test mirroring a simple modification
  $ echo a >> dir2/subdir/a
  $ hg commit -m "modify in dir2"
  mirrored changes in 'dir2/subdir/a' to 'dir1/a'
  $ hg diff --git -r .^ -r .
  diff --git a/dir1/a b/dir1/a
  --- a/dir1/a
  +++ b/dir1/a
  @@ -1,1 +1,2 @@
   a
  +a
  diff --git a/dir2/subdir/a b/dir2/subdir/a
  --- a/dir2/subdir/a
  +++ b/dir2/subdir/a
  @@ -1,1 +1,2 @@
   a
  +a

Test mirroring a simple delete
  $ hg rm dir1/a
  $ hg commit -m "remove in dir1"
  mirrored remove of 'dir1/a' to 'dir2/subdir/a'
  $ hg diff --git -r .^ -r .
  diff --git a/dir1/a b/dir1/a
  deleted file mode 100644
  --- a/dir1/a
  +++ /dev/null
  @@ -1,2 +0,0 @@
  -a
  -a
  diff --git a/dir2/subdir/a b/dir2/subdir/a
  deleted file mode 100644
  --- a/dir2/subdir/a
  +++ /dev/null
  @@ -1,2 +0,0 @@
  -a
  -a

Test conflicting edits
  $ mkdir dir1/
  $ mkdir -p dir2/subdir/
  $ echo a > dir1/a
  $ echo b > dir2/subdir/a
  $ hg commit -Am "add conflicting changes"
  adding dir1/a
  adding dir2/subdir/a
  abort: path 'dir1/a' needs to be mirrored to 'dir2/subdir/a', but the target already has pending changes
  [255]

Test non-conflicting edits
  $ echo a > dir2/subdir/a
  $ hg commit -Am "add non-conflicting changes"
  not mirroring 'dir1/a' to 'dir2/subdir/a'; it already matches
  not mirroring 'dir2/subdir/a' to 'dir1/a'; it already matches
  $ hg diff --git -r .^ -r .
  diff --git a/dir1/a b/dir1/a
  new file mode 100644
  --- /dev/null
  +++ b/dir1/a
  @@ -0,0 +1,1 @@
  +a
  diff --git a/dir2/subdir/a b/dir2/subdir/a
  new file mode 100644
  --- /dev/null
  +++ b/dir2/subdir/a
  @@ -0,0 +1,1 @@
  +a

Test non-conflicting deletes
  $ hg rm dir1/a dir2/subdir/a
  $ hg commit -Am "non-conflicting removes"
  not mirroring remove of 'dir1/a' to 'dir2/subdir/a'; it is already removed
  not mirroring remove of 'dir2/subdir/a' to 'dir1/a'; it is already removed
  $ hg diff --git -r .^ -r .
  diff --git a/dir1/a b/dir1/a
  deleted file mode 100644
  --- a/dir1/a
  +++ /dev/null
  @@ -1,1 +0,0 @@
  -a
  diff --git a/dir2/subdir/a b/dir2/subdir/a
  deleted file mode 100644
  --- a/dir2/subdir/a
  +++ /dev/null
  @@ -1,1 +0,0 @@
  -a

- Add it back for the next test
  $ mkdir dir1
  $ echo a > dir1/a
  $ hg commit -Am "add a back"
  adding dir1/a
  mirrored adding 'dir1/a' to 'dir2/subdir/a'

Test syncing a edit + rename
  $ echo b > dir1/a
  $ hg mv dir1/a dir1/b
  $ hg commit -m "edit and move a to b in dir1"
  mirrored copy 'dir1/a -> dir1/b' to 'dir2/subdir/a -> dir2/subdir/b'
  mirrored remove of 'dir1/a' to 'dir2/subdir/a'
  $ hg diff --git -r .^ -r .
  diff --git a/dir1/a b/dir1/b
  rename from dir1/a
  rename to dir1/b
  --- a/dir1/a
  +++ b/dir1/b
  @@ -1,1 +1,1 @@
  -a
  +b
  diff --git a/dir2/subdir/a b/dir2/subdir/b
  rename from dir2/subdir/a
  rename to dir2/subdir/b
  --- a/dir2/subdir/a
  +++ b/dir2/subdir/b
  @@ -1,1 +1,1 @@
  -a
  +b

Test amending a change where there has already been a sync before
  $ echo c > dir1/b
  $ hg commit --amend -m "amend b in dir1"
  mirrored changes in 'dir1/b' to 'dir2/subdir/b'
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/a6e4f018e982-f4dc39cf-amend-backup.hg (glob)
  $ hg diff --git -r .^ -r .
  diff --git a/dir1/a b/dir1/b
  rename from dir1/a
  rename to dir1/b
  --- a/dir1/a
  +++ b/dir1/b
  @@ -1,1 +1,1 @@
  -a
  +c
  diff --git a/dir2/subdir/a b/dir2/subdir/b
  rename from dir2/subdir/a
  rename to dir2/subdir/b
  --- a/dir2/subdir/a
  +++ b/dir2/subdir/b
  @@ -1,1 +1,1 @@
  -a
  +c

  $ cd ..

Test syncing multiple mirror groups across more than 2 directories
  $ rm -rf repo
  $ hg init repo
  $ cd repo
  $ cat >> .hg/hgrc <<EOF
  > [dirsync]
  > group1.dir1 = dir1/
  > group1.dir2 = dir2/
  > group1.dir3 = other/dir3
  > group2.dir1 = foo/dir1
  > group2.dir2 = foo/dir2
  > EOF
  $ mkdir -p dir1 foo/dir1
  $ echo a > dir1/a
  $ echo b > foo/dir1/a
  $ hg commit -Am "add stuff to two mirror groups"
  adding dir1/a
  adding foo/dir1/a
  mirrored adding 'dir1/a' to 'dir2/a'
  mirrored adding 'dir1/a' to 'other/dir3/a'
  mirrored adding 'foo/dir1/a' to 'foo/dir2/a'
  $ hg diff --git -r null -r .
  diff --git a/dir1/a b/dir1/a
  new file mode 100644
  --- /dev/null
  +++ b/dir1/a
  @@ -0,0 +1,1 @@
  +a
  diff --git a/dir2/a b/dir2/a
  new file mode 100644
  --- /dev/null
  +++ b/dir2/a
  @@ -0,0 +1,1 @@
  +a
  diff --git a/foo/dir1/a b/foo/dir1/a
  new file mode 100644
  --- /dev/null
  +++ b/foo/dir1/a
  @@ -0,0 +1,1 @@
  +b
  diff --git a/foo/dir2/a b/foo/dir2/a
  new file mode 100644
  --- /dev/null
  +++ b/foo/dir2/a
  @@ -0,0 +1,1 @@
  +b
  diff --git a/other/dir3/a b/other/dir3/a
  new file mode 100644
  --- /dev/null
  +++ b/other/dir3/a
  @@ -0,0 +1,1 @@
  +a

  $ cd ..

Test that rebasing applies the same change to both
  $ rm -rf repo
  $ hg init repo
  $ cd repo
  $ cat >> .hg/hgrc <<EOF
  > [dirsync]
  > group1.dir1 = dir1/
  > group1.dir2 = dir2/
  > EOF
  $ mkdir dir1 dir2
  $ echo a > dir1/a
  $ hg commit -Am "add dir1/a"
  adding dir1/a
  mirrored adding 'dir1/a' to 'dir2/a'
  $ echo x > unrelated
  $ hg commit -Am "add unrelated"
  adding unrelated
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo b > dir1/a
  $ hg commit --config extensions.dirsync=! -m "edit dir1/a with sync on"
  created new head
  $ hg rebase --config extensions.rebase= -d 1
  rebasing 2:70b4edc7f658 "edit dir1/a with sync on" (tip)
  mirrored changes in 'dir1/a' to 'dir2/a'
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/70b4edc7f658-c81f5ea9-backup.hg (glob)
  $ hg diff --git -r .^ -r .
  diff --git a/dir1/a b/dir1/a
  --- a/dir1/a
  +++ b/dir1/a
  @@ -1,1 +1,1 @@
  -a
  +b
  diff --git a/dir2/a b/dir2/a
  --- a/dir2/a
  +++ b/dir2/a
  @@ -1,1 +1,1 @@
  -a
  +b
