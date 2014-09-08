  $ extpath=$(dirname $TESTDIR)
  $ cp $extpath/tweakdefaults.py $TESTTMP # use $TESTTMP substitution in message
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > tweakdefaults=$TESTTMP/tweakdefaults.py
  > rebase=
  > EOF

Setup repo

  $ hg init repo
  $ cd repo
  $ touch a
  $ hg commit -Aqm a
  $ mkdir dir
  $ touch dir/b
  $ hg commit -Aqm b
  $ hg up -q 0
  $ echo x >> a
  $ hg commit -Aqm a2

Empty update fails

  $ hg up -q 0
  $ hg up
  abort: you must specify a destination to update to
  (if you're trying to move a bookmark forward, try 'hg rebase -d <destination>')
  [255]

  $ hg up -q -r 1
  $ hg log -r . -T '{rev}\n'
  1
  $ hg up -q 1
  $ hg log -r . -T '{rev}\n'
  1

Log is -f by default

  $ hg log -G -T '{rev} {desc}\n'
  @  1 b
  |
  o  0 a
  
  $ hg log -G -T '{rev} {desc}\n' --all
  o  2 a2
  |
  | @  1 b
  |/
  o  0 a
  
Log on dir's works

  $ hg log -T '{rev} {desc}\n' dir
  1 b

  $ hg log -T '{rev} {desc}\n' -I 'dir/*'
  1 b

Empty rebase fails

  $ hg rebase
  abort: you must specify a destination (-d) for the rebase
  [255]
  $ hg rebase -d 2
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/7b4cb4e1674c-backup.hg

Rebase fast forwards bookmark

  $ hg book -r 1 mybook
  $ hg up -q mybook
  $ hg log -G -T '{rev} {desc} {bookmarks}\n'
  @  1 a2 mybook
  |
  o  0 a
  
  $ hg rebase -d 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg log -G -T '{rev} {desc} {bookmarks}\n'
  @  2 b mybook
  |
  o  1 a2
  |
  o  0 a
  
