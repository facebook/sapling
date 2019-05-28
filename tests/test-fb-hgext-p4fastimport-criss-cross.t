  $ setconfig extensions.treemanifest=!
#require p4

  $ . $TESTDIR/p4setup.sh

populate the depot
  $ mkdir Main
  $ mkdir Main/b
  $ echo '1' > Main/a
  $ p4 add Main/a
  //depot/Main/a#1 - opened for add
  $ p4 submit -d'CL1(1)'
  Submitting change 1.
  Locking 1 files ...
  add //depot/Main/a#1
  Change 1 submitted.

  $ p4 edit Main/a
  //depot/Main/a#1 - opened for edit
  $ echo '4' > Main/a
  $ cat >desc <<EOF
  > Change: new
  > Client: $P4CLIENT
  > User: $USER
  > Status: new
  > Description: CL4(2)
  > Files:
  >     //depot/Main/a # edit
  > EOF
  $ p4 shelve -i < desc
  Change 2 created with 1 open file(s).
  Shelving files for change 2.
  edit //depot/Main/a#1
  Change 2 files shelved.
  $ p4 reopen -c default Main/a
  //depot/Main/a#1 - reopened; default change
  $ echo '3' > Main/a
  $ p4 submit -d'CL3(3)'
  Submitting change 3.
  Locking 1 files ...
  edit //depot/Main/a#2
  Change 3 submitted.
  $ cat Main/a
  3
  $ p4 unshelve -s 2 -c 2
  //depot/Main/a#1 - unshelved, opened for edit
  $ p4 shelve -c 2 -d
  Shelved change 2 deleted.
  $ p4 sync
  //depot/Main/a#2 - is opened and not being changed
  * //depot/Main/a - must resolve #2 before submitting (glob)
  $ p4 resolve -ay
  $TESTTMP/p4/Main/a - vs //depot/Main/a#2
  //hg-p4-import/Main/a - ignored //depot/Main/a
  $ p4 submit -c 2
  Submitting change 2.
  Locking 1 files ...
  edit //depot/Main/a#3
  Change 2 renamed change 4 and submitted.
  $ cat Main/a
  4

Import

  $ cd $hgwd
  $ hg init --config 'format.usefncache=False'
  $ hg p4seqimport -P $P4ROOT hg-p4-import
  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 3 changesets, 3 total revisions
  $ hg cat -r tip Main/a
  4
  stopping the p4 server
