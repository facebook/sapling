  $ echo "[extensions]" >> $HGRCPATH
  $ echo "remotenames=" >> $HGRCPATH
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
  $ hg serve -p 0 --port-file $TESTTMP/.port -d --pid-file=../hg.pid
  $ HGPORT=`cat $TESTTMP/.port`
  $ cd ..
  $ cat hg.pid >> $DAEMON_PIDS

clone
  $ hg clone http://localhost:$HGPORT/ copy 2>&1
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 4 changes to 4 files
  new changesets 8b6053c928fe
  updating to branch default
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cd copy

add a commit to the clone
  $ echo alpha > alpha
  $ hg add alpha
  $ hg ci -m 'add alpha'

verify that the branchheads are stored properly
  $ hg pull
  pulling from http://localhost:$HGPORT/ (glob)
  searching for changes
  no changes found
  $ hg log --graph
  @  changeset:   1:610fbbf9c9f6
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     add alpha
  |
  o  changeset:   0:8b6053c928fe
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     1
  
