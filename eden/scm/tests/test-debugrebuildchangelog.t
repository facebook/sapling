#chg-compatible

  $ configure modern

  $ newrepo server
  $ setconfig treemanifest.server=true
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
  $ hg clone --uncompressed ssh://user@dummy/server client
  streaming all changes
  6 files to transfer, 901 bytes of data
  transferred 901 bytes in 0.0 seconds (880 KB/sec)
  searching for changes
  no changes found
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

Rebuild:

  $ hg debugrebuildchangelog --trace
  read 3 draft commits
  fetching changelog
  6 files to transfer, 901 bytes of data
  transferred 901 bytes in 0.0 seconds (880 KB/sec)
  fetching selected remote bookmarks
  recreated 3 draft commits
  changelog rebuilt

  $ hg log -r 'all()' --git -T '{desc}' -G
  o  E
  |
  | o  F
  | |
  o |  D
  | |
  @ |  C
  |/
  o  B
  |
  o  A
  
