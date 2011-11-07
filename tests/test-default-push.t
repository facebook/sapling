  $ hg init a

  $ echo a > a/a
  $ hg --cwd a ci -Ama
  adding a

  $ hg clone a c
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg clone a b
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ echo b >> b/a
  $ hg --cwd b ci -mb

Push should push to 'default' when 'default-push' not set:

  $ hg --cwd b push
  pushing to $TESTTMP/a (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files

Push should push to 'default-push' when set:

  $ echo 'default-push = ../c' >> b/.hg/hgrc
  $ hg --cwd b push
  pushing to $TESTTMP/c (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files

