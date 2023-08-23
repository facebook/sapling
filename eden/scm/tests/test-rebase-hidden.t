#debugruntest-compatible

  $ configure modern
  $ enable rebase

Simple case:
  $ newrepo simple
  $ drawdag << 'EOS'
  > d
  > | c
  > | |
  > | b
  > |/
  > a
  > EOS
  $ hg hide $c
  hiding commit a82ac2b38757 "c"
  1 changeset hidden
  $ hg log -G -T '{desc}'
  o  d
  │
  │ o  b
  ├─╯
  o  a
  $ hg goto $b
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg rebase -d $d
  rebasing 488e1b7e7341 "b"
  $ hg log -G -T '{desc}'
  @  b
  │
  o  d
  │
  o  a
