  $ setconfig extensions.treemanifest=!
Set up extension and repos

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "remotenames=" >> $HGRCPATH
  $ echo "color=" >> $HGRCPATH
  $ echo "[color]" >> $HGRCPATH
  $ echo "log.remotebookmark = yellow" >> $HGRCPATH
  $ echo "log.remotebranch = red" >> $HGRCPATH
  $ echo "log.hoistedname = blue" >> $HGRCPATH
  $ hg init repo1
  $ cd repo1
  $ echo a > a
  $ hg add a
  $ hg commit -qm 'a'
  $ hg boo bm2
  $ cd ..
  $ hg clone repo1 repo2
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd repo2
  $ hg bookmark local

Test colors

  $ hg log --color=always -l 1
  \x1b[0;33mchangeset:   0:cb9a9f314b8b\x1b[0m (esc)
  bookmark:    local
  tag:         tip
  \x1b[0;33mbookmark:    default/bm2\x1b[0m (esc)
  \x1b[0;34mhoistedname: bm2\x1b[0m (esc)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
  
