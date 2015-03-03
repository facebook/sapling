Set up extension and repos
  $ echo "[extensions]" >> $HGRCPATH
  $ echo "remotenames=$(dirname $TESTDIR)/remotenames.py" >> $HGRCPATH
  $ hg init repo1
  $ cd repo1
  $ echo a > a
  $ hg add a
  $ hg commit -qm 'a'
  $ hg boo bm1
  $ hg boo bm2
  $ cd ..
  $ hg clone repo1 repo2
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd repo2
  $ hg log -l 1 -T '{node|short} {remotenames}\n'
  cb9a9f314b8b default/bm1 default/bm2 default/default

Test renaming

  $ rm .hg/remotenames
  $ echo "[remotenames]" >> $HGRCPATH
  $ echo "rename.default = remote" >> $HGRCPATH
  $ hg pull
  pulling from $TESTTMP/repo1
  searching for changes
  no changes found
  $ hg log -l 1 -T '{node|short} {remotenames}\n'
  cb9a9f314b8b remote/bm1 remote/bm2 remote/default

Test hoisting basics
  $ hg book
  no bookmarks set
  $ hg debugnamecomplete
  default
  remote/bm1
  remote/bm2
  remote/default
  tip
  $ echo "[remotenames]" >> $HGRCPATH
  $ echo "hoistremotebookmarks = remote" >> $HGRCPATH
  $ hg debugnamecomplete
  bm1
  bm2
  default
  remote/bm1
  remote/bm2
  remote/default
  tip

Test hoisting name lookup
  $ hg log -r remote/bm1 -T '{node|short} - {bookmarks} - {remotebookmarks}\n'
  cb9a9f314b8b -  - remote/bm1 remote/bm2
  $ hg log -r bm2 -T '{node|short} - {bookmarks} - {remotebookmarks}\n'
  cb9a9f314b8b -  - remote/bm1 remote/bm2
