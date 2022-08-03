#chg-compatible

  $ setconfig workingcopy.ruststatus=False
  $ configure modern

File to dir:

  $ newrepo
  $ echo A | drawdag
  $ hg up -q $A
  $ rm A
  $ mkdir -p A/A
  $ touch A/A/A
  $ hg revert .
  reverting A
  $ cat A
  A (no-eol)

File to parent dir:

  $ newrepo
  $ drawdag << 'EOS'
  > A  # A/D/D/D/1=1
  > EOS
  $ hg up -q $A
  $ rm -rf D/D
  $ echo 2 > D/D
  $ hg revert .
  reverting D/D/D/1
