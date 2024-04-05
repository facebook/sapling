#debugruntest-compatible

#require eden

setup backing repo

  $ eagerepo
  $ newrepo e1
  $ drawdag << 'EOS'
  > E  # bookmark master = E
  > |
  > D
  > |
  > C  # bookmark stable = C
  > |
  > B
  > |
  > A
  > EOS

test eden clone

  $ eden clone $TESTTMP/e1 $TESTTMP/e2 | dos2unix
  Cloning new repository at $TESTTMP/e2...
  Success.  Checked out commit 9bc730a1
  $ eden list
  $TESTTMP/e2
  $ cd $TESTTMP/e2
  $ ls -a
  .eden
  .hg
  A
  B
  C
  D
  E
  $ hg go $A
  update complete
  $ ls
  A
