#require p4

  $ . $TESTDIR/p4setup.sh

populate the depot
  $ mkdir Main
  $ echo a > 'Main/@'
  $ echo b > 'Main/#'
  $ echo c > 'Main/*'
  $ echo d > 'Main/%'
  $ echo d > 'Main/a'
  $ p4 add -f 'Main/@' 'Main/#' 'Main/*' 'Main/%' 'Main/a'
  //depot/Main/%40#1 - opened for add
  //depot/Main/%23#1 - opened for add
  //depot/Main/%2A#1 - opened for add
  //depot/Main/%25#1 - opened for add
  //depot/Main/a#1 - opened for add
  $ p4 submit -d initial
  Submitting change 1.
  Locking 5 files ...
  add //depot/Main/%23#1
  add //depot/Main/%25#1
  add //depot/Main/%2A#1
  add //depot/Main/%40#1
  add //depot/Main/a#1
  Change 1 submitted.

Simple import

  $ cd $hgwd
  $ hg init --config 'format.usefncache=False'
  $ hg p4seqimport -P $P4ROOT hg-p4-import

Verify

  $ hg manifest -r 0
  Main/#
  Main/%
  Main/*
  Main/@
  Main/a

End Test

  stopping the p4 server
