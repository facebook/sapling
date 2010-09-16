  $ hg init a
  $ hg clone a b
  updating to branch default
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd a
  $ echo '[paths]' >> .hg/hgrc
  $ echo 'dupe = ../b' >> .hg/hgrc
  $ hg in dupe
  comparing with .*/test-paths.t/b
  no changes found
  [1]
  $ cd ..
  $ hg -R a in dupe
  comparing with .*/test-paths.t/b
  no changes found
  [1]
  $ true
