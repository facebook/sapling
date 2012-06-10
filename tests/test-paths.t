  $ hg init a
  $ hg clone a b
  updating to branch default
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd a
  $ echo '[paths]' >> .hg/hgrc
  $ echo 'dupe = ../b' >> .hg/hgrc
  $ echo 'expand = $SOMETHING/bar' >> .hg/hgrc
  $ hg in dupe
  comparing with $TESTTMP/b (glob)
  no changes found
  [1]
  $ cd ..
  $ hg -R a in dupe
  comparing with $TESTTMP/b (glob)
  no changes found
  [1]
  $ cd a
  $ hg paths
  dupe = $TESTTMP/b (glob)
  expand = $TESTTMP/a/$SOMETHING/bar (glob)
  $ SOMETHING=foo hg paths
  dupe = $TESTTMP/b (glob)
  expand = $TESTTMP/a/foo/bar (glob)
#if msys
  $ SOMETHING=//foo hg paths
  dupe = $TESTTMP/b (glob)
  expand = /foo/bar
#else
  $ SOMETHING=/foo hg paths
  dupe = $TESTTMP/b (glob)
  expand = /foo/bar
#endif
  $ hg paths -q
  dupe
  expand
  $ hg paths dupe
  $TESTTMP/b (glob)
  $ hg paths -q dupe
  $ hg paths unknown
  not found!
  [1]
  $ hg paths -q unknown
  [1]
  $ cd ..

'file:' disables [paths] entries for clone destination

  $ cat >> $HGRCPATH <<EOF
  > [paths]
  > gpath1 = http://hg.example.com
  > EOF

  $ hg clone a gpath1
  abort: cannot create new http repository
  [255]

  $ hg clone a file:gpath1
  updating to branch default
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd gpath1
  $ hg -q id
  000000000000

  $ cd ..
