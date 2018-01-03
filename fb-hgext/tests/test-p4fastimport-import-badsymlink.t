#require p4

  $ . $TESTDIR/p4setup.sh

populate the depot

  $ mkdir Main
  $ mkdir Main/b
  $ echo a > Main/a
  $ echo c > Main/b/c
  $ ln -s b Main/d
  $ p4 add Main/a Main/b/c Main/d
  //depot/Main/a#1 - opened for add
  //depot/Main/b/c#1 - opened for add
  //depot/Main/d#1 - opened for add
  $ p4 submit -d initial
  Submitting change 1.
  Locking 3 files ...
  add //depot/Main/a#1
  add //depot/Main/b/c#1
  add //depot/Main/d#1
  Change 1 submitted.
  $ rm Main/d
  $ mkdir -p Main/d
  $ echo d > Main/d/d
  $ p4 add Main/d/d
  //depot/Main/d/d#1 - opened for add
  $ p4 submit -d two
  Submitting change 2.
  Locking 1 files ...
  add //depot/Main/d/d#1
  Change 2 submitted.
  $ p4 files ...
  //depot/Main/a#1 - add change 1 (text)
  //depot/Main/b/c#1 - add change 1 (text)
  //depot/Main/d#1 - add change 1 (symlink)
  //depot/Main/d/d#1 - add change 2 (text)

According to P4, Main/d/d is a file inside Main/d, which is a symlink(!)

  $ cd $hgwd
  $ hg init --config 'format.usefncache=False'
  $ hg p4fastimport --bookmark master -P $P4ROOT hg-p4-import
  warning: ignoring Main/d/d because it's under a symlink (Main/d)
  $ hg update
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ ls -l Main
  total 8
  -rw-r--r-- 1 .* a (re)
  drwxr-xr-x 2 .* b (re)
  lrwxrwxrwx 1 .* d -> b (re)

Repeat with a file nested under several directories

  $ cd $p4wd
  $ mkdir -p Main/d/e
  $ echo e > Main/d/e/e
  $ p4 add Main/d/e/e
  //depot/Main/d/e/e#1 - opened for add
  $ p4 submit -d three
  Submitting change 3.
  Locking 1 files ...
  add //depot/Main/d/e/e#1
  Change 3 submitted.
  $ p4 files ...
  //depot/Main/a#1 - add change 1 (text)
  //depot/Main/b/c#1 - add change 1 (text)
  //depot/Main/d#1 - add change 1 (symlink)
  //depot/Main/d/d#1 - add change 2 (text)
  //depot/Main/d/e/e#1 - add change 3 (text)

Second import

  $ cd $hgwd
  $ hg p4fastimport --bookmark master -P $P4ROOT hg-p4-import
  warning: ignoring Main/d/e/e because it's under a symlink (Main/d)
  $ hg update
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

End Test

  stopping the p4 server
