  $ echo "[extensions]" >> $HGRCPATH
  $ echo "remotenames=$(echo $(dirname $TESTDIR))/remotenames.py" >> $HGRCPATH
 
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
  $ hg serve -p $HGPORT -d --pid-file=../hg.pid
  $ cd ..
  $ cat hg.pid >> $DAEMON_PIDS

clone
  $ hg clone http://localhost:$HGPORT/ copy 2>&1 | \
  > sed -e 's/[0-9][0-9.]*/XXX/g' -e 's/[KM]\(B\/sec\)/X\1/'
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added XXX changesets with XXX changes to XXX files
  updating to branch default
  XXX files updated, XXX files merged, XXX files removed, XXX files unresolved

  $ cd copy

add a commit to the clone
  $ echo alpha > alpha
  $ hg add alpha
  $ hg ci -m 'add alpha'

verify that the branchheads are stored properly
  $ hg pull | sed "s/$HGPORT//"
  pulling from http://localhost:/
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
     branch:      default/default
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     1
  
