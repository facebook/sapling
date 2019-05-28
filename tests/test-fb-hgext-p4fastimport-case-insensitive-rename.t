  $ setconfig extensions.treemanifest=!
#require p4

  $ P4DOPTS=-C1
  $ . $TESTDIR/p4setup.sh

populate the depot
  $ mkdir Main
  $ echo a > Main/a
  $ p4 add Main/a
  //depot/Main/a#1 - opened for add
  $ p4 submit -d initial
  Submitting change 1.
  Locking 1 files ...
  add //depot/Main/a#1
  Change 1 submitted.
  $ p4 edit Main/a
  //depot/Main/a#1 - opened for edit
  $ p4 move Main/a Main/b
  //depot/Main/b#1 - moved from //depot/Main/a#1
  $ p4 submit -d moveway
  Submitting change 2.
  Locking 2 files ...
  move/delete //depot/Main/a#2
  move/add //depot/Main/b#1
  Change 2 submitted.
  $ p4 edit Main/b
  //depot/Main/b#1 - opened for edit
  $ p4 move Main/b Main/A
  //depot/Main/A#2 - moved from //depot/Main/b#1
  $ p4 submit -d moveback
  Submitting change 3.
  Locking 2 files ...
  move/add //depot/Main/A#3
  move/delete //depot/Main/b#2
  Change 3 submitted.

import

  $ cd $hgwd
  $ hg init --config 'format.usefncache=False'
  $ hg p4seqimport -P $P4ROOT hg-p4-import

Verify

  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  3 files, 3 changesets, 3 total revisions

Update

  $ hg manifest -r 0
  Main/a
  $ hg manifest -r 1
  Main/b
  $ hg manifest -r 2
  Main/A
  $ hg cat Main/a -r 0
  a
  $ hg cat Main/b -r 1
  a
  $ hg cat Main/A -r 2
  a
  stopping the p4 server
