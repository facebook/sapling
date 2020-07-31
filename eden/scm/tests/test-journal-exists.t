#chg-compatible

  $ disable treemanifest
  $ hg init
  $ echo a > a
  $ hg ci -Am0
  adding a

  $ hg -q clone . foo

  $ touch .hg/store/journal

  $ echo foo > a
  $ hg ci -Am0
  abort: abandoned transaction found!
  (run 'hg recover' to clean up transaction)
  [255]

  $ hg recover
  rolling back interrupted transaction

Check that zero-size journals are correctly aborted:

#if unix-permissions no-root
  $ hg bundle -qa repo.hg
  $ chmod -w foo/.hg/store/00changelog.i

XXX: This can be a better error message.

  $ hg -R foo unbundle repo.hg
  adding changesets
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 1 files
  abort: Permission denied (os error 13)
  [255]

  $ if test -f foo/.hg/store/journal; then echo 'journal exists :-('; fi
#endif

