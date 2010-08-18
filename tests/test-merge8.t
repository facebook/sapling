Test for changeset ba7c74081861
(update dirstate correctly for non-branchmerge updates)
  $ hg init a
  $ cd a
  $ echo a > a
  $ hg add a
  $ hg commit -m a
  $ cd ..
  $ hg clone a b
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd a
  $ hg mv a b
  $ hg commit -m move
  $ echo b >> b
  $ hg commit -m b
  $ cd ../b
  $ hg pull ../a
  pulling from ../a
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 1 files
  (run 'hg update' to get a working copy)
  $ hg update
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
