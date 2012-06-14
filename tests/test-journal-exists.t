  $ hg init
  $ echo a > a
  $ hg ci -Am0
  adding a

  $ hg -q clone . foo

  $ touch .hg/store/journal

  $ echo foo > a
  $ hg ci -Am0
  abort: abandoned transaction found - run hg recover!
  [255]

  $ hg recover
  rolling back interrupted transaction
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 1 changesets, 1 total revisions

Check that zero-size journals are correctly aborted:

#if unix-permissions
  $ hg bundle -qa repo.hg
  $ chmod -w foo/.hg/store/00changelog.i

  $ hg -R foo unbundle repo.hg
  adding changesets
  abort: Permission denied: $TESTTMP/foo/.hg/store/.00changelog.i-* (glob)
  [255]

  $ if test -f foo/.hg/store/journal; then echo 'journal exists :-('; fi
#endif

