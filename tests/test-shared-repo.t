  $ echo "[extensions]" >> $HGRCPATH
  $ echo "share=" >> $HGRCPATH
  $ echo "remotenames=`dirname $TESTDIR`/remotenames.py" >> $HGRCPATH
  $ hg init upstream
  $ cd upstream
  $ touch file0
  $ hg add file0
  $ hg commit -m "file0"
  $ cd ..
  $ hg clone upstream primary
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd primary
  $ hg log --graph
  @  changeset:   0:d26a60f4f448
     tag:         tip
     branch:      default/default
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     file0
  
  $ cd ..
  $ hg share primary secondary
  updating working directory
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd secondary
  $ hg log --graph
  @  changeset:   0:d26a60f4f448
     tag:         tip
     branch:      default/default
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     file0
  
