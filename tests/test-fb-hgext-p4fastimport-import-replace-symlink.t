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
  $ p4 delete Main/d
  //depot/Main/d#1 - opened for delete
  $ p4 submit -d delete
  Submitting change 2.
  Locking 1 files ...
  delete //depot/Main/d#2
  Change 2 submitted.
  $ echo d > Main/d
  $ p4 add Main/d
  //depot/Main/d#2 - opened for add
  $ p4 submit -d replaced
  Submitting change 3.
  Locking 1 files ...
  add //depot/Main/d#3
  Change 3 submitted.
  $ p4 files ...
  //depot/Main/a#1 - add change 1 (text)
  //depot/Main/b/c#1 - add change 1 (text)
  //depot/Main/d#3 - add change 3 (text)

Now import it

  $ cd $hgwd
  $ hg init --config 'format.usefncache=False'
  $ hg p4seqimport --bookmark master -P $P4ROOT hg-p4-import
  $ hg update -r 0
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ ls -l Main/d
  lrwx.* 1 .* Main/d -> b (re)
  $ hg update -r tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ ls -l Main/d
  -rw-.* 1 .* Main/d (re)

End Test

  stopping the p4 server
