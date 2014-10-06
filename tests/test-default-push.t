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

Push should provide a hint when both 'default' and 'default-push' not set:
  $ cd c
  $ hg push --config paths.default=
  pushing to default-push
  abort: default repository not configured!
  (see the "path" section in "hg help config")
  [255]

  $ cd ..

Push should push to 'default' when 'default-push' not set:

  $ hg --cwd b push
  pushing to $TESTTMP/a (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files

Push should push to 'default-push' when set:

  $ echo '[paths]' >> b/.hg/hgrc
  $ echo 'default-push = ../c' >> b/.hg/hgrc
  $ hg --cwd b push
  pushing to $TESTTMP/c (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
