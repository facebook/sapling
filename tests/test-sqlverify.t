  $ . "$TESTDIR/library.sh"

Populate the db with an initial commit

  $ initserver master masterrepo
  $ cd master
  $ echo a > a
  $ hg commit -Aqm 'add a'
  $ echo b > b
  $ hg commit -Aqm 'add b'
  $ hg up -q 0
  $ echo c > c
  $ hg commit -Aqm 'add c'

Run with a correct revlog

  $ hg sqlverify
  Verification passed

Run with incorrect local revlogs

  $ hg strip -r 1 --config hgsql.bypass=True
  saved backup bundle to $TESTTMP/master/.hg/strip-backup/7c3bad9141dc-81844e36-backup.hg (glob)
  $ hg unbundle --config hgsql.bypass=True $TESTTMP/master/.hg/strip-backup/7c3bad9141dc-81844e36-backup.hg
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)

  $ hg log -r tip --forcesync -T '{desc}\n'
  add b
  $ hg sqlverify 2>&1 | grep Corruption
  hgext_hgsql.CorruptionException: '*' with linkrev *, disk does not match mysql (glob)
