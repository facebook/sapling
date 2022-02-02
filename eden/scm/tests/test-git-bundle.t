#chg-compatible
#require git no-windows

  $ . $TESTDIR/git.sh

Prepare bundle

  $ hg init --git gitrepo1
  $ cd gitrepo1
  $ drawdag << 'EOS'
  >   D
  >   |
  > B C  Y
  >  \|  |
  >   A  X
  > EOS
  $ hg bookmark -r $B book-B
  $ hg bookmark -r $B book-B2

  $ hg bundle -r $B+$D+$Y --base $A $TESTTMP/bundle

Apply bundle in another repo

  $ cd
  $ hg init --git gitrepo2
  $ cd gitrepo2
  $ drawdag << 'EOS'
  > A
  > EOS
  $ hg unbundle -u $TESTTMP/bundle
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg log -Gr: -T '{desc} {bookmarks}'
  @  D
  │
  o  C
  │
  │ o  Y
  │ │
  │ o  X
  │
  │ o  B book-B book-B2
  ├─╯
  o  A
  
