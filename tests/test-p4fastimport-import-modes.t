#require p4

  $ . $TESTDIR/p4setup.sh

populate the depot
  $ mkdir Main
  $ mkdir Main/b
  $ echo a > Main/a
  $ ln -s Main/a Main/b/c
  $ echo d > Main/d
  $ chmod +x Main/d
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

  $ p4 edit Main/a Main/b/c Main/d
  //depot/Main/a#1 - opened for edit
  //depot/Main/b/c#1 - opened for edit
  //depot/Main/d#1 - opened for edit
  $ echo a >> Main/a
  $ echo d >> Main/d
  $ p4 submit -d second
  Submitting change 2.
  Locking 3 files ...
  edit //depot/Main/a#2
  edit //depot/Main/b/c#2
  edit //depot/Main/d#2
  Change 2 submitted.

Simple import

  $ cd $hgwd
  $ hg init --config 'format.usefncache=False'
  $ hg p4fastimport --debug -P $P4ROOT hg-p4-import
  loading changelist numbers.
  2 changelists to import.
  loading list of files.
  3 files to import.
  reading filelog * (glob)
  reading filelog * (glob)
  reading filelog * (glob)
  importing repository.
  writing filelog: b789fdd96dc2, p1 000000000000, linkrev 0, 2 bytes, src: *, path: Main/a (glob)
  writing filelog: a80d06849b33, p1 b789fdd96dc2, linkrev 1, 4 bytes, src: *, path: Main/a (glob)
  writing filelog: 8aa36f7e9a8d, p1 000000000000, linkrev 0, 7 bytes, src: *, path: Main/b/c (glob)
  writing filelog: ee47780ebabc, p1 8aa36f7e9a8d, linkrev 1, 7 bytes, src: *, path: Main/b/c (glob)
  writing filelog: a9092a3d84a3, p1 000000000000, linkrev 0, 2 bytes, src: *, path: Main/d (glob)
  writing filelog: f83f0637e55e, p1 a9092a3d84a3, linkrev 1, 4 bytes, src: *, path: Main/d (glob)
  changelist 1: writing manifest. node: 9b06e09b6cf9 p1: 000000000000 p2: 000000000000 linkrev: 0
  changelist 1: writing changelog: initial
  changelist 2: writing manifest. node: d5f0551e02e2 p1: 9b06e09b6cf9 p2: 000000000000 linkrev: 1
  changelist 2: writing changelog: second
  updating the branch cache (?)
  2 revision(s), 3 file(s) imported.

Verify

  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  3 files, 2 changesets, 6 total revisions

  $ hg update tip
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg --debug manifest
  a80d06849b333b8a3d5c445f8ba3142010dcdc9e 644   Main/a
  ee47780ebabc4dd227d21ef3b71ca3ab381eb4cf 644 @ Main/b/c
  f83f0637e55e3c48e9922f14a016761626d79d3d 755 * Main/d

End Test

  stopping the p4 server
