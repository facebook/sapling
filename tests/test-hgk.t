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
  $ echo b > b
  $ hg ci -Am addb
  adding b
  $ hg log -T '{node}\n'
  102a90ea7b4a3361e4082ed620918c261189a36a
  07f4944404050f47db2e5c5071e0e84e7a27bba9

  $ hg debug-diff-tree 07f494440405 102a90ea7b4a
  :000000 100664 000000000000 1e88685f5dde N	b	b
  $ hg debug-diff-tree 07f494440405 102a90ea7b4a --patch
  diff --git a/b b/b
  new file mode 100644
  --- /dev/null
  +++ b/b
  @@ -0,0 +1,1 @@
  +b

Ensure that diff-tree output isn't affected by diffopts
  $ hg --config diff.noprefix=True debug-diff-tree 07f494440405 102a90ea7b4a
  :000000 100664 000000000000 1e88685f5dde N	b	b
  $ hg --config diff.noprefix=True debug-diff-tree --patch 07f494440405 102a90ea7b4a
  diff --git a/b b/b
  new file mode 100644
  --- /dev/null
  +++ b/b
  @@ -0,0 +1,1 @@
  +b

  $ cd ..
