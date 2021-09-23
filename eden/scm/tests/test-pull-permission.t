#chg-compatible
  $ setconfig experimental.allowfilepeer=True

  $ disable treemanifest
#require unix-permissions no-root

  $ hg init a
  $ cd a
  $ echo foo > b
  $ hg add b
  $ hg ci -m "b"

  $ chmod -w .hg/store

  $ cd ..

  $ hg clone a b
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ chmod +w a/.hg/store # let test clean up

  $ cd b
  $ hg verify
  warning: verify does not actually check anything in this repo

  $ cd ..
