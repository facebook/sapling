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
  $ hg p4fastimport --debug -P $P4ROOT hg-p4-import
  loading changelist numbers.
  3 changelists to import.
  loading list of files.
  3 files to import.
  reading filelog * (glob)
  reading filelog * (glob)
  reading filelog * (glob)
  importing repository.
  case conflict: //depot/Main/A and //depot/Main/a
  writing filelog: b789fdd96dc2, p1 000000000000, linkrev 2, 2 bytes, src: *, path: Main/A (glob)
  writing filelog: b789fdd96dc2, p1 000000000000, linkrev 0, 2 bytes, src: *, path: Main/a (glob)
  writing filelog: b789fdd96dc2, p1 000000000000, linkrev 1, 2 bytes, src: *, path: Main/b (glob)
  changelist 1: writing manifest. node: f495e209f723 p1: 000000000000 p2: 000000000000 linkrev: 0
  changelist 1: writing changelog: initial
  changelist 2: writing manifest. node: 510da33a44e3 p1: f495e209f723 p2: 000000000000 linkrev: 1
  changelist 2: writing changelog: moveway
  changelist 3: writing manifest. node: 6541d210de72 p1: 510da33a44e3 p2: 000000000000 linkrev: 2
  changelist 3: writing changelog: moveback
  updating the branch cache (?)
  3 revision(s), 3 file(s) imported.

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
  $ hg update -r 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat Main/a
  a
  $ hg update -r 1
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ cat Main/b
  a
  $ hg update -r 2
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ cat Main/A
  a
  stopping the p4 server
