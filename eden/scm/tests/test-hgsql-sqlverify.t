#chg-compatible

  $ . "$TESTDIR/hgsql/library.sh"
  $ setconfig extensions.treemanifest=!

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

  $ hg debugstrip -r 1 --config hgsql.bypass=True
  saved backup bundle to $TESTTMP/master/.hg/strip-backup/7c3bad9141dc-81844e36-backup.hg (glob)
  $ hg unbundle --config hgsql.bypass=True $TESTTMP/master/.hg/strip-backup/7c3bad9141dc-81844e36-backup.hg
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files

  $ hg log -r tip --forcesync -T '{desc}\n'
  add b
  $ hg sqlverify >$TESTTMP/sqlverify.out 2>&1 || true
  $ grep Corruption $TESTTMP/sqlverify.out || cat $TESTTMP/sqlverify.out
  CorruptionException: * with linkrev *, disk does not match mysql (glob)

  $ hg debugstrip -q -r 1: --config hgsql.bypass=True --no-backup
  $ hg log -r tip --forcesync -T '\n'
  

Run with correct changelog but incorrect revlogs
  $ hg sqlverify
  Verification passed
  $ mkdir .hg/store/backups
  $ cp .hg/store/00changelog* .hg/store/backups/
  $ echo >> a
  $ hg commit -qm "modify a" --config hgsql.bypass=True
  $ cp .hg/store/backups/* .hg/store/
  $ hg sqlverify
  corruption: 'data/a.i:eb2346e7cf59326667069bf8647698840687803d' with linkrev 3 exists on local disk, but not in sql
  corruption: '00manifest.i:05e2c764eb21dd7c597aab767cec621af1292344' with linkrev 3 exists on local disk, but not in sql
  abort: Verification failed
  [255]
