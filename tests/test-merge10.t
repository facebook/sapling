Test for changeset 9fe267f77f56ff127cf7e65dc15dd9de71ce8ceb
(merge correctly when all the files in a directory are moved
but then local changes are added in the same directory)

  $ hg init a
  $ cd a
  $ mkdir -p testdir
  $ echo a > testdir/a
  $ hg add testdir/a
  $ hg commit -m a
  $ cd ..

  $ hg clone a b
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd a
  $ echo alpha > testdir/a
  $ hg commit -m remote-change
  $ cd ..

  $ cd b
  $ mkdir testdir/subdir
  $ hg mv testdir/a testdir/subdir/a
  $ hg commit -m move
  $ mkdir newdir
  $ echo beta > newdir/beta
  $ hg add newdir/beta
  $ hg commit -m local-addition
  $ hg pull ../a
  pulling from ../a
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hg up -C 2
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg merge
  merging testdir/subdir/a and testdir/a to testdir/subdir/a
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg stat
  M testdir/subdir/a
  $ hg diff --nodates
  diff -r bc21c9773bfa testdir/subdir/a
  --- a/testdir/subdir/a
  +++ b/testdir/subdir/a
  @@ -1,1 +1,1 @@
  -a
  +alpha

  $ cd ..
