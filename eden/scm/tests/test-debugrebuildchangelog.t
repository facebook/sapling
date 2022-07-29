#chg-compatible
  $ configure modern
  $ enable shelve

  $ newrepo server --config extensions.treemanifest=$TESTDIR/../edenscm/hgext/treemanifestserver.py
  $ setconfig treemanifest.server=true extensions.treemanifest=$TESTDIR/../edenscm/hgext/treemanifestserver.py
  $ drawdag << 'EOS'
  > C
  > |
  > B
  > |
  > A
  > EOS
  $ hg bookmark -r $C master

Clone:

  $ cd $TESTTMP
  $ hg clone --no-shallow --stream ssh://user@dummy/server client
  streaming all changes
  4 files to transfer, * bytes of data (glob)
  transferred * bytes in 0.0 seconds (* KB/sec) (glob)
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

Add drafts:

  $ cd client
  $ drawdag << 'EOS'
  > E
  > |
  > D          F
  > |          |
  > master   desc(B)
  > EOS

Prepare another test case backed by a server repo that speaks EdenAPI

  $ newremoterepo
  $ setconfig paths.default=test:e

  $ drawdag << 'EOS'
  > E
  > |
  > D
  > |
  > C F
  > |/
  > B
  > |
  > A
  > EOS

  $ hg bookmark -r $D book-D
  $ hg push -r $C --to master --create
  pushing rev 26805aba1e60 to destination test:e bookmark master
  searching for changes
  exporting bookmark master

Prepare to test shelve:

  $ hg up -q 'desc(F)'
  $ echo 2 >> F
  $ hg shelve
  shelved as default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

Rebuild using segmented changelog

  $ hg debugrebuildchangelog
  backed up 4 commits to commits-4-0000.bak
  imported clone data with tip 26805aba1e600a82e93661149f2313866a221a7b
  pulling latest commits
  recreating 4 local commits
  changelog rebuilt

  $ hg log -r 'all()' --git -T '{desc} {remotenames} {bookmarks}' -G
  @  F
  │
  │ o  E
  │ │
  │ o  D  book-D
  │ │
  │ o  C remote/master
  ├─╯
  o  B
  │
  o  A
  
Unshelve works:

  $ hg unshelve
  unshelving change 'default'

  $ cat F
  F2

Test pull error does not end up with a broken repo:

  $ FAILPOINTS=debugrebuildchangelog-before-pull=return hg debugrebuildchangelog
  backed up 3 commits to commits-3-0000.bak
  imported clone data with tip 26805aba1e600a82e93661149f2313866a221a7b
  pulling latest commits
  restoring changelog from previous state
  abort: failpoint 'debugrebuildchangelog-before-pull' set by FAILPOINTS
  [255]

  $ hg log -r 'all()' --git -T '{desc} {remotenames} {bookmarks}' -G
  @  F
  │
  │ o  E
  │ │
  │ o  D  book-D
  │ │
  │ o  C remote/master
  ├─╯
  o  B
  │
  o  A
  
