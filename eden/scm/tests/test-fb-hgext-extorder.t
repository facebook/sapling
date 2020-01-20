#chg-compatible

Set up repository

  $ hg init repo
  $ cd repo
  $ enable extorder rebase histedit
  $ setconfig extensions.dummyext1="$TESTDIR/dummyext1.py"
  $ setconfig extensions.dummyext2="$TESTDIR/dummyext2.py"

Simple Dependency

  $ hg id
  ext1: uisetup
  ext2: uisetup
  ext1: extsetup
  ext2: extsetup
  000000000000

  $ readconfig <<EOF
  > [extorder]
  > dummyext1 = dummyext2
  > preferfirst = histedit
  > preferlast = rebase
  > EOF

  $ hg id
  ext1: uisetup
  ext2: uisetup
  ext2: extsetup
  ext1: extsetup
  000000000000

Conflicting deps

  $ setconfig extorder.dummyext2=dummyext1
  $ hg id > out.txt 2>&1
  [1]
  $ grep MercurialExtOrderException: < out.txt
  MercurialExtOrderException: extorder: conflicting extension order
