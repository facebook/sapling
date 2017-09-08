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
  $ p4 delete Main/a
  //depot/Main/a#1 - opened for delete
  $ p4 submit -ddelete
  Submitting change 2.
  Locking 1 files ...
  delete //depot/Main/a#2
  Change 2 submitted.
  $ echo a > Main/A
  $ p4 add Main/A
  //depot/Main/A#2 - opened for add
  $ p4 submit -d 'add with case-inensitivity match'
  Submitting change 3.
  Locking 1 files ...
  add //depot/Main/A#3
  Change 3 submitted.

import

  $ cd $hgwd
  $ hg init --config 'format.usefncache=False'
  $ hg p4fastimport --debug -P $P4ROOT hg-p4-import
  loading changelist numbers.
  3 changelists to import.
  loading list of files.
  2 files to import.
  reading filelog * (glob)
  reading filelog * (glob)
  importing repository.
  case conflict: //depot/Main/A and //depot/Main/a
  writing filelog: b789fdd96dc2, p1 000000000000, linkrev 2, 2 bytes, src: *, path: Main/A (glob)
  writing filelog: b789fdd96dc2, p1 000000000000, linkrev 0, 2 bytes, src: *, path: Main/a (glob)
  changelist 1: writing manifest. node: f495e209f723 p1: 000000000000 p2: 000000000000 linkrev: 0
  changelist 1: writing changelog: initial
  changelist 2: writing manifest. node: * p1: * p2: 000000000000 linkrev: 1 (glob)
  updating the branch cache (?)
  changelist 2: writing changelog: delete
  changelist 3: writing manifest. node: * p1: * p2: 000000000000 linkrev: 2 (glob)
  changelist 3: writing changelog: add with case-inensitivity match
  updating the branch cache (?)
  3 revision(s), 2 file(s) imported.

Verify

  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  2 files, 3 changesets, 2 total revisions

Update

  $ hg update -r 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat Main/a
  a
  $ hg update -r 1
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg update -r 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat Main/A
  a
  stopping the p4 server
