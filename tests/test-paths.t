  $ hg init a
  $ hg clone a b
  updating to branch default
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd a
  $ echo '[paths]' >> .hg/hgrc
  $ echo 'dupe = ../b' >> .hg/hgrc
  $ echo 'expand = $SOMETHING/bar' >> .hg/hgrc
  $ hg in dupe
  comparing with $TESTTMP/b
  no changes found
  [1]
  $ cd ..
  $ hg -R a in dupe
  comparing with $TESTTMP/b
  no changes found
  [1]
  $ cd a
  $ hg paths
  dupe = $TESTTMP/b
  expand = $TESTTMP/a/$SOMETHING/bar
  $ SOMETHING=foo hg paths
  dupe = $TESTTMP/b
  expand = $TESTTMP/a/foo/bar
  $ SOMETHING=/foo hg paths
  dupe = $TESTTMP/b
  expand = /foo/bar
