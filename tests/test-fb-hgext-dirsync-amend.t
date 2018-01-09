  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > dirsync=
  > EOF

  $ hg init repo
  $ cd repo
  $ cat >> .hg/hgrc <<EOF
  > [ui]
  > verbose=true
  > [dirsync]
  > sync1.1=dir1/
  > sync1.2=dir2/subdir/
  > EOF

Add multiple files
  $ mkdir dir1
  $ echo a > dir1/a
  $ echo b > dir1/b
  $ hg commit -Am "Adding a and b"
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
  committed changeset 0:32bc2a06fd26
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

Include only changes to particular file
  $ echo a >> dir1/a
  $ echo b >> dir1/b
  $ hg commit --amend -I dir1/a
  mirrored changes in 'dir1/a' to 'dir2/subdir/a'
  amending changeset 32bc2a06fd26
  committing files:
  dir1/a
  dir1/b
  dir2/subdir/a
  dir2/subdir/b
  committing manifest
  committing changelog
  1 changesets found
  uncompressed size of bundle content:
       227 (changelog)
       326 (manifests)
       132  dir1/a
       132  dir1/b
       139  dir2/subdir/a
       139  dir2/subdir/b
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/32bc2a06fd26-fb68e7cc-amend.hg (glob)
  1 changesets found
  uncompressed size of bundle content:
       281 (changelog)
       326 (manifests)
       134  dir1/a
       132  dir1/b
       141  dir2/subdir/a
       139  dir2/subdir/b
  adding branch
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 4 changes to 4 files
  committed changeset 0:e9cce3b53a7c

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
  mirrored changes in 'dir1/b' to 'dir2/subdir/b'
  amending changeset e9cce3b53a7c
  committing files:
  dir1/a
  dir1/b
  dir2/subdir/a
  dir2/subdir/b
  committing manifest
  committing changelog
  1 changesets found
  uncompressed size of bundle content:
       281 (changelog)
       326 (manifests)
       134  dir1/a
       132  dir1/b
       141  dir2/subdir/a
       139  dir2/subdir/b
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/e9cce3b53a7c-5d332711-amend.hg (glob)
  1 changesets found
  uncompressed size of bundle content:
       281 (changelog)
       326 (manifests)
       134  dir1/a
       134  dir1/b
       141  dir2/subdir/a
       141  dir2/subdir/b
  adding branch
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 4 changes to 4 files
  committed changeset 0:a70e8a6cacdd

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

Exclude changes to particular file
  $ echo b >> dir1/b
  $ hg commit --amend -X dir1/a
  mirrored changes in 'dir1/b' to 'dir2/subdir/b'
  amending changeset a70e8a6cacdd
  committing files:
  dir1/a
  dir1/b
  dir2/subdir/a
  dir2/subdir/b
  committing manifest
  committing changelog
  1 changesets found
  uncompressed size of bundle content:
       281 (changelog)
       326 (manifests)
       134  dir1/a
       134  dir1/b
       141  dir2/subdir/a
       141  dir2/subdir/b
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/a70e8a6cacdd-136cbdc1-amend.hg (glob)
  1 changesets found
  uncompressed size of bundle content:
       281 (changelog)
       326 (manifests)
       134  dir1/a
       136  dir1/b
       141  dir2/subdir/a
       143  dir2/subdir/b
  adding branch
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 4 changes to 4 files
  committed changeset 0:4af805a433df
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

Check the addremove flag
  $ echo c > dir1/c
  $ rm dir1/a
  $ hg commit --amend -A
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
  1 changesets found
  uncompressed size of bundle content:
       281 (changelog)
       326 (manifests)
       134  dir1/a
       136  dir1/b
       141  dir2/subdir/a
       143  dir2/subdir/b
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/4af805a433df-2246e36e-amend.hg (glob)
  1 changesets found
  uncompressed size of bundle content:
       281 (changelog)
       326 (manifests)
       136  dir1/b
       132  dir1/c
       143  dir2/subdir/b
       139  dir2/subdir/c
  adding branch
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 4 changes to 4 files
  committed changeset 0:55c6a18e7d57

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
