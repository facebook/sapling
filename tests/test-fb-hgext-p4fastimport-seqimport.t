#require p4

  $ . $TESTDIR/p4setup.sh

Populate depot
  $ mkdir Main
  $ echo a > Main/a
  $ echo b > Main/b
  $ p4 add Main/a Main/b
  //depot/Main/a#1 - opened for add
  //depot/Main/b#1 - opened for add
  $ p4 submit -d first
  Submitting change 1.
  Locking 2 files ...
  add //depot/Main/a#1
  add //depot/Main/b#1
  Change 1 submitted.

  $ p4 edit Main/a Main/b
  //depot/Main/a#1 - opened for edit
  //depot/Main/b#1 - opened for edit
  $ p4 move Main/a Main/amove
  //depot/Main/amove#1 - moved from //depot/Main/a#1
  $ echo bb >> Main/b
  $ echo c >> Main/c
  $ p4 add Main/c
  //depot/Main/c#1 - opened for add
  $ p4 submit -d second
  Submitting change 2.
  Locking 4 files ...
  move/delete //depot/Main/a#2
  move/add //depot/Main/amove#1
  edit //depot/Main/b#2
  add //depot/Main/c#1
  Change 2 submitted.

Run seqimport
  $ cd $hgwd
  $ hg init --config 'format.usefncache=False'
  $ hg p4seqimport --debug -P $P4ROOT $P4CLIENT
  loading changelist numbers.
  2 changelists to import.
  importing CL1
  added: Main/a Main/b
  importing CL2
  added: Main/amove Main/c
  removed: Main/a
  $ cat Main/amove
  a
  $ cat Main/b
  b
  bb
  $ cat Main/c
  c

End Test
  stopping the p4 server
