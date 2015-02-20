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

Test hoisting basics
  $ hg book
  no bookmarks set
  $ hg debugnamecomplete
  default
  default/bm1
  default/bm2
  default/default
  tip
  $ echo "[remotenames]" >> $HGRCPATH
  $ echo "hoistremotebookmarks = default" >> $HGRCPATH
  $ hg debugnamecomplete
  bm1
  bm2
  default
  default/bm1
  default/bm2
  default/default
  tip

Test hoisting name lookup
  $ hg log -r default/bm1 -T '{node|short} - {bookmarks} - {remotebookmarks}\n'
  cb9a9f314b8b -  - default/bm1 default/bm2
  $ hg log -r bm2 -T '{node|short} - {bookmarks} - {remotebookmarks}\n'
  cb9a9f314b8b -  - default/bm1 default/bm2
