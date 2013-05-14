http://mercurial.selenic.com/bts/issue1175

  $ hg init
  $ touch a
  $ hg ci -Am0
  adding a

  $ hg mv a a1
  $ hg ci -m1

  $ hg co 0
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved

  $ hg mv a a2
  $ hg up
  note: possible conflict - a was renamed multiple times to:
   a2
   a1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg ci -m2

  $ touch a
  $ hg ci -Am3
  adding a

  $ hg mv a b
  $ hg ci -Am4 a

  $ hg ci --debug --traceback -Am5 b
  b
   b: searching for copy revision for a
   b: copy a:b80de5d138758541c5f05265ad144ab9fa86d1db
  committed changeset 5:732aafbecb501a198b3cc9323ad3899ff04ccf95

  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  4 files, 6 changesets, 4 total revisions

  $ hg export --git tip
  # HG changeset patch
  # User test
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID 732aafbecb501a198b3cc9323ad3899ff04ccf95
  # Parent  1d1625283f71954f21d14c3d44d0ad3c019c597f
  5
  
  diff --git a/b b/b
  new file mode 100644

