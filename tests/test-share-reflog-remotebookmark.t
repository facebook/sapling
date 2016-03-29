  $ extpath=`dirname $TESTDIR`
  $ cp $extpath/reflog.py $TESTTMP # use $TESTTMP substitution in message
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > reflog=$TESTTMP/reflog.py
  > remotenames=
  > [remotenames]
  > rename.default=remote
  > EOF

  $ hg init repo
  $ cd repo
  $ hg bookmark bm
  $ touch file0
  $ hg commit -Am 'file0 added'
  adding file0

  $ cd ..
  $ hg clone repo cloned1
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd cloned1
  $ hg reflog remote/bm
  Previous locations of 'remote/bm':
  5640b525682e  clone repo cloned1

  $ cd ..
  $ hg-new-workdir cloned1 cloned2
  Setting up configuration...
  Updating new repository...
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  Set up new Mercurial Working Directory in 'cloned2' based on 'cloned1'...
  $ cd cloned2
  $ hg reflog remote/bm
  Previous locations of 'remote/bm':
  5640b525682e  clone repo cloned1

  $ cd ../repo
  $ touch file1
  $ hg commit -Am "file1 added"
  adding file1
  $ cd ../cloned1
  $ hg pull
  pulling from $TESTTMP/repo (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  (run 'hg update' to get a working copy)
  $ hg reflog remote/bm
  Previous locations of 'remote/bm':
  6432d239ac5d  pull
  5640b525682e  clone repo cloned1
  $ cd ../cloned2
  $ hg reflog remote/bm
  Previous locations of 'remote/bm':
  6432d239ac5d  pull
  5640b525682e  clone repo cloned1
