
  $ hg init test
  $ cd test
  $ echo foo>foo
  $ mkdir foo.d foo.d/bAr.hg.d foo.d/baR.d.hg
  $ echo foo>foo.d/foo
  $ echo bar>foo.d/bAr.hg.d/BaR
  $ echo bar>foo.d/baR.d.hg/bAR
  $ hg commit -A -m 1
  adding foo
  adding foo.d/bAr.hg.d/BaR
  adding foo.d/baR.d.hg/bAR
  adding foo.d/foo
  $ hg serve -p $HGPORT -d --pid-file=../hg1.pid -E ../error.log
  $ hg --config server.uncompressed=False serve -p $HGPORT1 -d --pid-file=../hg2.pid

Test server address cannot be reused

  $ hg serve -p $HGPORT1 2>&1
  abort: cannot start server at ':$HGPORT1': Address already in use
  [255]
  $ cd ..
  $ cat hg1.pid hg2.pid >> $DAEMON_PIDS

clone via stream

  $ hg clone --uncompressed http://localhost:$HGPORT/ copy 2>&1
  streaming all changes
  6 files to transfer, 606 bytes of data
  transferred * bytes in * seconds (*B/sec) (glob)
  updating to branch default
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg verify -R copy
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  4 files, 1 changesets, 4 total revisions

try to clone via stream, should use pull instead

  $ hg clone --uncompressed http://localhost:$HGPORT1/ copy2
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 4 changes to 4 files
  updating to branch default
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved

clone via pull

  $ hg clone http://localhost:$HGPORT1/ copy-pull
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 4 changes to 4 files
  updating to branch default
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg verify -R copy-pull
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  4 files, 1 changesets, 4 total revisions
  $ cd test
  $ echo bar > bar
  $ hg commit -A -d '1 0' -m 2
  adding bar
  $ cd ..

pull

  $ cd copy-pull
  $ echo '[hooks]' >> .hg/hgrc
  $ echo 'changegroup = python "$TESTDIR"/printenv.py changegroup' >> .hg/hgrc
  $ hg pull
  pulling from http://localhost:$HGPORT1/
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  changegroup hook: HG_NODE=5fed3813f7f5e1824344fdc9cf8f63bb662c292d HG_SOURCE=pull HG_URL=http://localhost:$HGPORT1/ 
  (run 'hg update' to get a working copy)
  $ cd ..

clone from invalid URL

  $ hg clone http://localhost:$HGPORT/bad
  abort: HTTP Error 404: Not Found
  [255]

check error log

  $ cat error.log
