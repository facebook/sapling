  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > strip =
  > writecg2 = $TESTDIR/../writecg2.py
  > EOF
  $ hg init repo
  $ cd repo
  $ touch a && hg add a && hg ci -ma
  $ touch b && hg add b && hg ci -mb
  $ touch c && hg add c && hg ci -mc
  $ hg log -T compact --graph
  @  2[tip]   991a3460af53   1970-01-01 00:00 +0000   test
  |    c
  |
  o  1   0e067c57feba   1970-01-01 00:00 +0000   test
  |    b
  |
  o  0   3903775176ed   1970-01-01 00:00 +0000   test
       a
  

unbundle should barf appropriately on not-a-bundle
  $ echo GIT123 > ../notabundle
  $ hg unbundle ../notabundle
  abort: ../notabundle: not a Mercurial bundle
  [255]

bundle by itself shouldn't be changegroup2
  $ hg bundle --base 0 --rev 2 ../bundle.bundle
  2 changesets found
  $ dd count=6 bs=1 if=../bundle.bundle 2>/dev/null
  HG10BZ (no-eol)

a strip bundle should be changegroup2
  $ hg strip 1
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/0e067c57feba-3c242e3d-backup.hg (glob)
  $ dd count=6 bs=1 if=.hg/strip-backup/0e067c57feba-3c242e3d-backup.hg 2>/dev/null
  HG20\x00\x00 (no-eol) (esc)

applying bundle1 should continue to work
  $ hg unbundle ../bundle.bundle
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  (run 'hg update' to get a working copy)
  $ hg log -T compact --graph
  o  2[tip]   991a3460af53   1970-01-01 00:00 +0000   test
  |    c
  |
  o  1   0e067c57feba   1970-01-01 00:00 +0000   test
  |    b
  |
  @  0   3903775176ed   1970-01-01 00:00 +0000   test
       a
  

... and via pull
  $ hg strip 1
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/0e067c57feba-3c242e3d-backup.hg (glob)
  $ hg pull ../bundle.bundle
  pulling from ../bundle.bundle
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  (run 'hg update' to get a working copy)

  $ hg strip 1
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/0e067c57feba-3c242e3d-backup.hg (glob)
  $ dd count=6 bs=1 if=.hg/strip-backup/0e067c57feba-3c242e3d-backup.hg 2>/dev/null
  HG20\x00\x00 (no-eol) (esc)

hg incoming on a changegroup2 should work
  $ hg incoming .hg/strip-backup/0e067c57feba-3c242e3d-backup.hg --traceback
  comparing with .hg/strip-backup/0e067c57feba-3c242e3d-backup.hg
  searching for changes
  changeset:   1:0e067c57feba
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     b
  
  changeset:   2:991a3460af53
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     c
  
applying a changegroup2 should work via unbundle
  $ hg unbundle .hg/strip-backup/0e067c57feba-3c242e3d-backup.hg
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  (run 'hg update' to get a working copy)

... and via pull
  $ hg strip 1
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/0e067c57feba-3c242e3d-backup.hg (glob)
  $ dd count=6 bs=1 if=.hg/strip-backup/0e067c57feba-3c242e3d-backup.hg 2>/dev/null
  HG20\x00\x00 (no-eol) (esc)
  $ hg pull .hg/strip-backup/0e067c57feba-3c242e3d-backup.hg
  pulling from .hg/strip-backup/0e067c57feba-3c242e3d-backup.hg
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  (run 'hg update' to get a working copy)

amends should also be cg2
  $ hg up 2
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ touch d && hg add d && hg ci --amend -mcd
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/991a3460af53-046ba7e5-amend-backup.hg (glob)
  $ dd count=6 bs=1 if=.hg/strip-backup/991a3460af53-046ba7e5-amend-backup.hg 2>/dev/null
  HG20\x00\x00 (no-eol) (esc)

turn on bundle2
  $ cat >> $HGRCPATH <<EOF
  > [experimental]
  > bundle2-exp = True
  > strip-bundle2-version = 02
  > EOF

incoming should still work
  $ hg incoming .hg/strip-backup/991a3460af53-046ba7e5-amend-backup.hg
  comparing with .hg/strip-backup/991a3460af53-046ba7e5-amend-backup.hg
  searching for changes
  changeset:   3:991a3460af53
  parent:      1:0e067c57feba
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     c
  
  changeset:   4:e5a1db54cb59
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     temporary amend commit for 991a3460af53
  

strip should produce bundle2
  $ hg strip 1
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/0e067c57feba-1954568c-backup.hg (glob)
  $ hg debugbundle .hg/strip-backup/0e067c57feba-1954568c-backup.hg
  Stream params: {'Compression': 'BZ'}
  changegroup -- "{'version': '02'}"
      0e067c57feba1a5694ca4844f05588bb1bf82342
      b2a74d690cb63a443d20de84bdc9eb5a7ddbedac
  $ hg incoming .hg/strip-backup/0e067c57feba-1954568c-backup.hg
  comparing with .hg/strip-backup/0e067c57feba-1954568c-backup.hg
  searching for changes
  changeset:   1:0e067c57feba
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     b
  
  changeset:   2:b2a74d690cb6
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     cd
  
  $ hg pull .hg/strip-backup/0e067c57feba-1954568c-backup.hg
  pulling from .hg/strip-backup/0e067c57feba-1954568c-backup.hg
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 3 changes to 3 files
  (run 'hg update' to get a working copy)
