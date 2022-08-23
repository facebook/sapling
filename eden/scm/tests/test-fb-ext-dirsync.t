#chg-compatible

  $ enable dirsync

  $ hg init repo
  $ cd repo
  $ setconfig ui.verbose=true
  $ readconfig <<EOF
  > [dirsync]
  > sync1.1=dir1/
  > sync1.2=dir2/subdir/
  > EOF

Test mirroring a simple add

  $ mkdir dir1/
  $ echo a > dir1/a
  $ hg add dir1/a
  adding dir1/a
  $ hg commit --traceback -m "add in dir1"
  mirrored adding 'dir1/a' to 'dir2/subdir/a'
  committing files:
  dir1/a
  dir2/subdir/a
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
  committing files:
  dir1/a
  dir2/subdir/a
  committing manifest
  committing changelog
  committed * (glob)
  $ hg diff --git -r ".^" -r .
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
  removing dir1/a
  $ hg commit -m "remove in dir1"
  mirrored remove of 'dir1/a' to 'dir2/subdir/a'
  committing files:
  committing manifest
  committing changelog
  committed * (glob)
  $ hg diff --git -r ".^" -r .
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
  adding dir1/a
  adding dir2/subdir/a
  not mirroring 'dir1/a' to 'dir2/subdir/a'; it already matches
  not mirroring 'dir2/subdir/a' to 'dir1/a'; it already matches
  committing files:
  dir1/a
  dir2/subdir/a
  committing manifest
  committing changelog
  committed * (glob)
  $ hg diff --git -r ".^" -r .
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
  removing dir1/a
  removing dir2/subdir/a
  $ hg commit -Am "non-conflicting removes"
  not mirroring remove of 'dir1/a' to 'dir2/subdir/a'; it is already removed
  not mirroring remove of 'dir2/subdir/a' to 'dir1/a'; it is already removed
  committing files:
  committing manifest
  committing changelog
  committed * (glob)
  $ hg diff --git -r ".^" -r .
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
  committing files:
  dir1/a
  dir2/subdir/a
  committing manifest
  committing changelog
  committed * (glob)

Test syncing a edit + rename
  $ echo b > dir1/a
  $ hg mv dir1/a dir1/b
  moving dir1/a to dir1/b (glob)
  $ hg commit -m "edit and move a to b in dir1"
  mirrored copy 'dir1/a -> dir1/b' to 'dir2/subdir/a -> dir2/subdir/b'
  mirrored remove of 'dir1/a' to 'dir2/subdir/a'
  committing files:
  dir1/b
  dir2/subdir/b
  committing manifest
  committing changelog
  committed * (glob)
  $ hg diff --git -r ".^" -r .
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
  amending changeset * (glob)
  mirrored copy 'dir1/a -> dir1/b' to 'dir2/subdir/a -> dir2/subdir/b'
  committing files:
  dir1/b
  dir2/subdir/b
  committing manifest
  committing changelog
  committed * (glob)
  $ hg diff --git -r ".^" -r .
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
  $ readconfig <<EOF
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
  $ readconfig <<EOF
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
  $ hg up .^
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo b > dir1/a
  $ hg commit --config extensions.dirsync=! -m "edit dir1/a with sync on"
  $ cp -R . ../repo1
  $ hg rebase --config extensions.rebase= -d 'max(desc(add))'
  rebasing * "edit dir1/a with sync on" (glob)
  mirrored changes in 'dir1/a' to 'dir2/a'
  $ hg diff --git -r ".^" -r .
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

  $ cd ..

The same test as the above. But uses in-memory rebase
  $ cd repo1
  $ setconfig rebase.experimental.inmemory=True
  $ hg rebase --config extensions.rebase= -d 'max(desc(add))'
  rebasing * "edit dir1/a with sync on" (glob)
  mirrored changes in 'dir1/a' to 'dir2/a'
  $ hg diff --git -r ".^" -r .
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

  $ cd ..

Test committing part of the working copy
  $ rm -rf repo
  $ hg init repo
  $ cd repo
  $ readconfig <<EOF
  > [dirsync]
  > group1.dir1 = dir1/
  > group1.dir2 = dir2/
  > EOF
  $ mkdir dir1 dir2
  $ echo a > dir1/a
  $ echo b > dir1/b
  $ hg add dir1
  adding dir1/a
  adding dir1/b
  $ hg commit -Am "add dir1/a" "re:dir1/a"
  mirrored adding 'dir1/a' to 'dir2/a'
  $ hg status
  A dir1/b
  $ hg log -r . --stat
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add dir1/a
  
   dir1/a |  1 +
   dir2/a |  1 +
   2 files changed, 2 insertions(+), 0 deletions(-)
  

  $ echo a >> dir2/a
  $ hg commit --amend -m "add dir1/a" dir2/a
  mirrored adding 'dir2/a' to 'dir1/a'
  $ hg status
  A dir1/b
  $ hg log -r . --stat
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add dir1/a
  
   dir1/a |  2 ++
   dir2/a |  2 ++
   2 files changed, 4 insertions(+), 0 deletions(-)
  

  $ echo a >> dir1/a
  $ hg commit --amend -m "add dir1/a" dir2/a
  nothing changed
  [1]

  $ hg commit --amend -m "add dir1/a"
  mirrored adding 'dir1/a' to 'dir2/a'
  mirrored adding 'dir1/b' to 'dir2/b'
  $ hg status
  $ hg log -r . --stat
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add dir1/a
  
   dir1/a |  3 +++
   dir1/b |  1 +
   dir2/a |  3 +++
   dir2/b |  1 +
   4 files changed, 8 insertions(+), 0 deletions(-)
  

- Add it back for the next test
  $ echo a > dir1/a
  $ hg commit -m "add a back" --config ui.verbose=False
  mirrored changes in 'dir1/a' to 'dir2/a'

Test quiet non-conflicting edits
  $ echo aa > dir1/a
  $ echo aa > dir2/a
  $ hg commit -m "add non-conflicting changes" --config ui.verbose=True
  not mirroring 'dir1/a' to 'dir2/a'; it already matches
  not mirroring 'dir2/a' to 'dir1/a'; it already matches
  committing files:
  dir1/a
  dir2/a
  committing manifest
  committing changelog
  committed * (glob)
  $ echo aaa > dir1/a
  $ echo aaa > dir2/a
  $ hg commit -m "add non-conflicting changes" --config ui.verbose=False
  $ hg diff --git -r ".^" -r .
  diff --git a/dir1/a b/dir1/a
  --- a/dir1/a
  +++ b/dir1/a
  @@ -1,1 +1,1 @@
  -aa
  +aaa
  diff --git a/dir2/a b/dir2/a
  --- a/dir2/a
  +++ b/dir2/a
  @@ -1,1 +1,1 @@
  -aa
  +aaa

  $ cd ..

Test deleting file with missing mirror
  $ rm -rf repo
  $ hg init repo
  $ cd repo
  $ mkdir dir1
  $ echo a > dir1/a
  $ hg add dir1
  adding dir1/a
  $ hg commit -m 'add dir1/a'
  $ readconfig <<EOF
  > [dirsync]
  > group1.dir1 = dir1/
  > group1.dir2 = dir2/
  > EOF
  $ hg rm dir1/a
  $ hg status
  R dir1/a
  $ hg commit -m 'rm dir1/a'
  mirrored remove of 'dir1/a' to 'dir2/a'
  $ hg diff --git -r '.^' -r .
  diff --git a/dir1/a b/dir1/a
  deleted file mode 100644
  --- a/dir1/a
  +++ /dev/null
  @@ -1,1 +0,0 @@
  -a

  $ cd ..

Test modifying file with missing mirror
  $ rm -rf repo
  $ hg init repo
  $ cd repo
  $ mkdir dir1
  $ echo a > dir1/a
  $ hg add dir1
  adding dir1/a
  $ hg commit -m 'add dir1/a'
  $ readconfig <<EOF
  > [dirsync]
  > group1.dir1 = dir1/
  > group1.dir2 = dir2/
  > EOF
  $ echo a2 > dir1/a
  $ hg status
  M dir1/a
  $ hg commit -m 'modify dir1/a'
  mirrored changes in 'dir1/a' to 'dir2/a'
  $ cat dir2/a
  a2
  $ hg diff --git -r '.^' -r .
  diff --git a/dir1/a b/dir1/a
  --- a/dir1/a
  +++ b/dir1/a
  @@ -1,1 +1,1 @@
  -a
  +a2
  diff --git a/dir2/a b/dir2/a
  new file mode 100644
  --- /dev/null
  +++ b/dir2/a
  @@ -0,0 +1,1 @@
  +a2

  $ cd ..

Test updating missing mirror
  $ rm -rf repo
  $ hg init repo
  $ cd repo
  $ mkdir dir1
  $ echo a > dir1/a
  $ hg add dir1
  adding dir1/a
  $ hg commit -m 'add dir1/a'
  $ readconfig <<EOF
  > [dirsync]
  > group1.dir1 = dir1/
  > group1.dir2 = dir2/
  > EOF
  $ mkdir dir2
  $ echo a2 > dir2/a
  $ hg add dir2
  adding dir2/a
  $ hg status
  A dir2/a
  $ hg commit -m 'add dir2/a'
  mirrored adding 'dir2/a' to 'dir1/a'
  $ hg diff --git -r '.^' -r .
  diff --git a/dir1/a b/dir1/a
  --- a/dir1/a
  +++ b/dir1/a
  @@ -1,1 +1,1 @@
  -a
  +a2
  diff --git a/dir2/a b/dir2/a
  new file mode 100644
  --- /dev/null
  +++ b/dir2/a
  @@ -0,0 +1,1 @@
  +a2

  $ cd ..

Dont mirror during shelve
  $ rm -rf repo
  $ hg init repo
  $ cd repo
  $ enable shelve
  $ readconfig <<EOF
  > [dirsync]
  > group1.dir1 = dir1/
  > group1.dir2 = dir2/
  > EOF
  $ mkdir dir1
  $ echo a > dir1/a
  $ hg add dir1
  adding dir1/a
  $ hg commit -m 'add dir1/a'
  mirrored adding 'dir1/a' to 'dir2/a'
  $ echo a >> dir1/a
  $ hg shelve
  shelved as default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg status
  $ hg unshelve
  unshelving change 'default'
  $ hg status
  M dir1/a

  $ cd ..

Test .hgdirsync in the working copy

  $ rm -rf repo
  $ hg init repo
  $ cd repo
  $ readconfig <<EOF
  > [dirsync]
  > group1.dir1 = dir1/
  > group1.dir2 = dir2/
  > EOF
  $ cat >> .hgdirsync <<EOF
  > group1.dir3 = dir3/
  > group2.dir1 = dir4/
  > group2.dir2 = dir5/
  > EOF
  $ mkdir dir2 dir5
  $ echo a > dir2/a
  $ echo b > dir5/b
  $ hg commit -m init -A dir2/a dir5/b
  mirrored adding 'dir2/a' to 'dir1/a'
  mirrored adding 'dir2/a' to 'dir3/a'
  mirrored adding 'dir5/b' to 'dir4/b'
  $ hg log -p -r .
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     init
  
  diff -r 000000000000 -r * dir1/a (glob)
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/dir1/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  diff -r 000000000000 -r * dir2/a (glob)
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/dir2/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  diff -r 000000000000 -r * dir3/a (glob)
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/dir3/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  diff -r 000000000000 -r * dir4/b (glob)
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/dir4/b	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +b
  diff -r 000000000000 -r * dir5/b (glob)
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/dir5/b	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +b
  

Change .hgdirsync in the working copy affects what will be synced

  $ rm .hgdirsync

  $ echo c > dir2/c
  $ echo d > dir4/d
  $ hg commit -m subdir -A dir2/c dir4/d
  mirrored adding 'dir2/c' to 'dir1/c'

  $ cat >> .hgdirsync <<EOF
  > group1.dir6 = dir6/
  > group1.dir7 = dir7/
  > EOF

  $ echo c >> dir2/c
  $ hg commit -m 'modify group1'
  mirrored changes in 'dir2/c' to 'dir1/c'
  mirrored changes in 'dir2/c' to 'dir6/c'
  mirrored changes in 'dir2/c' to 'dir7/c'

Only the ".hgdirsync" at the top of the repo is effective

  $ cd dir1
  $ cat >> .hgdirsync <<'EOF'
  > group1.dir8 = dir8/
  > group1.dir9 = dir9/
  > EOF
  $ echo c >> c
  $ hg commit -m 'modify group1 again'
  mirrored changes in 'dir1/c' to 'dir2/c'
  mirrored changes in 'dir1/c' to 'dir6/c'
  mirrored changes in 'dir1/c' to 'dir7/c'

  $ cd ../..

Rule order matters. Only the first one gets executed.

  $ hg init $TESTTMP/repo-order1
  $ cd $TESTTMP/repo-order1
  $ cat >> .hgdirsync <<'EOF'
  > a.dir1 = a/
  > a.dir2 = b/
  > c.dir1 = a/c/
  > c.dir2 = c/
  > EOF
  $ mkdir -p a/c
  $ echo 1 > a/c/1
  $ hg commit -m 'order test' -A a
  adding a/c/1
  mirrored adding 'a/c/1' to 'b/c/1'

  $ hg init $TESTTMP/repo-order2
  $ cd $TESTTMP/repo-order2
  $ cat >> .hgdirsync <<'EOF'
  > c.dir1 = a/c/
  > c.dir2 = c/
  > a.dir1 = a/
  > a.dir2 = b/
  > EOF
  $ mkdir -p a/c
  $ echo 1 > a/c/1
  $ hg commit -m 'order test' -A a
  adding a/c/1
  mirrored adding 'a/c/1' to 'c/1'

Test excluding a subdirectory from dirsync
  $ hg init $TESTTMP/exclusion
  $ cd $TESTTMP/exclusion
  $ cat >> .hgdirsync <<'EOF'
  > a.dir1 = a/
  > exclude.a.dir1 = a/excl
  > a.dir2 = b/
  > exclude.a.dir2 = b/excl
  > EOF
  $ mkdir -p a/c
  $ mkdir -p a/excl
  $ echo 1 > a/c/1
  $ echo 2 > a/excl/2
  $ hg commit -m 'exclusion test' -A a
  adding a/c/1
  adding a/excl/2
  mirrored adding 'a/c/1' to 'b/c/1'

Test that excludes override all other rules
  $ hg init $TESTTMP/exclusion-override
  $ cd $TESTTMP/exclusion-override
  $ cat >> .hgdirsync <<'EOF'
  > a.dir1 = a/
  > exclude.a.dir1 = a/excl
  > a.dir2 = b/
  > exclude.a.dir2 = b/excl
  > b.dir1 = a/
  > b.dir2 = b/
  > c.dir1 = a/excl/foo
  > c.dir2 = b/excl/foo
  > EOF
  $ mkdir -p a/c
  $ mkdir -p a/excl/foo
  $ echo 1 > a/c/1
  $ echo 2 > a/excl/foo/2
  $ hg commit -m 'exclusion test' -A a
  adding a/c/1
  adding a/excl/foo/2
  mirrored adding 'a/c/1' to 'b/c/1'

Test that excludes only work when specified for every destination
  $ hg init $TESTTMP/exclusion-total
  $ cd $TESTTMP/exclusion-total
  $ cat >> .hgdirsync <<'EOF'
  > a.dir1 = a/
  > a.dir2 = b/
  > exclude.a.dir2 = b/excl
  > EOF
  $ mkdir -p a/c
  $ mkdir -p a/excl
  $ echo 1 > a/c/1
  $ echo 2 > a/excl/2
  $ hg commit -m 'exclusion test' -A a
  adding a/c/1
  adding a/excl/2
  mirrored adding 'a/c/1' to 'b/c/1'
  mirrored adding 'a/excl/2' to 'b/excl/2'

Match and exclude rules can match individual files
  $ newrepo sync-individual-files
  $ readconfig <<EOF
  > [dirsync]
  > group1.dir1 = somedir/
  > group1.dir2 = elsewhere/
  > singlefile.dir1 = a/myfile.txt
  > singlefile.dir2 = b/foo.txt
  > rootfile.dir1 = toplevel.txt
  > rootfile.dir2 = x/y/z/foo.txt
  > toroot.dir1 = a/root.txt
  > toroot.dir2 = root.txt
  > exclude.group1_path = somedir/readme.txt
  > EOF
  $ mkdir somedir
  $ echo a > somedir/a
  $ echo b > somedir/readme.txt
  $ mkdir a
  $ echo c > a/myfile.txt
  $ echo d > toplevel.txt
  $ echo e > a/root.txt
  $ echo f > toplevel.txt
  $ echo g > other.txt
  $ hg add somedir a other.txt toplevel.txt
  adding a/myfile.txt
  adding a/root.txt
  adding somedir/a
  adding somedir/readme.txt
  $ hg commit -m 'test commit' --traceback
  mirrored adding 'a/myfile.txt' to 'b/foo.txt'
  mirrored adding 'a/root.txt' to 'root.txt'
  mirrored adding 'somedir/a' to 'elsewhere/a'
  mirrored adding 'toplevel.txt' to 'x/y/z/foo.txt'
  $ hg diff --git -r null -r .
  diff --git a/a/myfile.txt b/a/myfile.txt
  new file mode 100644
  --- /dev/null
  +++ b/a/myfile.txt
  @@ -0,0 +1,1 @@
  +c
  diff --git a/a/root.txt b/a/root.txt
  new file mode 100644
  --- /dev/null
  +++ b/a/root.txt
  @@ -0,0 +1,1 @@
  +e
  diff --git a/b/foo.txt b/b/foo.txt
  new file mode 100644
  --- /dev/null
  +++ b/b/foo.txt
  @@ -0,0 +1,1 @@
  +c
  diff --git a/elsewhere/a b/elsewhere/a
  new file mode 100644
  --- /dev/null
  +++ b/elsewhere/a
  @@ -0,0 +1,1 @@
  +a
  diff --git a/other.txt b/other.txt
  new file mode 100644
  --- /dev/null
  +++ b/other.txt
  @@ -0,0 +1,1 @@
  +g
  diff --git a/root.txt b/root.txt
  new file mode 100644
  --- /dev/null
  +++ b/root.txt
  @@ -0,0 +1,1 @@
  +e
  diff --git a/somedir/a b/somedir/a
  new file mode 100644
  --- /dev/null
  +++ b/somedir/a
  @@ -0,0 +1,1 @@
  +a
  diff --git a/somedir/readme.txt b/somedir/readme.txt
  new file mode 100644
  --- /dev/null
  +++ b/somedir/readme.txt
  @@ -0,0 +1,1 @@
  +b
  diff --git a/toplevel.txt b/toplevel.txt
  new file mode 100644
  --- /dev/null
  +++ b/toplevel.txt
  @@ -0,0 +1,1 @@
  +f
  diff --git a/x/y/z/foo.txt b/x/y/z/foo.txt
  new file mode 100644
  --- /dev/null
  +++ b/x/y/z/foo.txt
  @@ -0,0 +1,1 @@
  +f

