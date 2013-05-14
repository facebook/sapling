Minimal hgk check

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "hgk=" >> $HGRCPATH
  $ hg init repo
  $ cd repo
  $ echo a > a
  $ hg ci -Am adda
  adding a
  $ hg debug-cat-file commit 0
  tree a0c8bcbbb45c
  parent 000000000000
  author test 0 0
  revision 0
  branch default
  phase draft
  
  adda

  $ cd ..
