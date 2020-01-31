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
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 1 changesets, 1 total revisions

Check that zero-size journals are correctly aborted:

#if unix-permissions no-root
  $ hg bundle -qa repo.hg
  $ chmod -w foo/.hg/store/00changelog.i

  $ hg -R foo unbundle repo.hg
  adding changesets
  abort: Permission denied: $TESTTMP/foo/.hg/store/.00changelog.i-* (glob)
  (current process runs with uid 42)
  ($TESTTMP/foo/.hg/store/.00changelog.i*: mode 0o52, uid 42, gid 42) (glob)
  ($TESTTMP/foo/.hg/store: mode 0o52, uid 42, gid 42)
  [255]

  $ if test -f foo/.hg/store/journal; then echo 'journal exists :-('; fi
#endif

