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
  $ hg p4fastimport --debug -P $P4ROOT hg-p4-import
  loading changelist numbers.
  1 changelists to import.
  loading list of files.
  5 files to import.
  reading filelog * (glob)
  reading filelog * (glob)
  reading filelog * (glob)
  reading filelog * (glob)
  reading filelog * (glob)
  importing repository.
  writing filelog: 1e88685f5dde, p1 000000000000, linkrev 0, 2 bytes, src: *, path: Main/# (glob)
  writing filelog: a9092a3d84a3, p1 000000000000, linkrev 0, 2 bytes, src: *, path: Main/% (glob)
  writing filelog: 149da44f2a4e, p1 000000000000, linkrev 0, 2 bytes, src: *, path: Main/* (glob)
  writing filelog: b789fdd96dc2, p1 000000000000, linkrev 0, 2 bytes, src: *, path: Main/@ (glob)
  writing filelog: a9092a3d84a3, p1 000000000000, linkrev 0, 2 bytes, src: *, path: Main/a (glob)
  changelist 1: writing manifest. node: edfad00a2e2d p1: 000000000000 p2: 000000000000 linkrev: 0
  changelist 1: writing changelog: initial
  updating the branch cache
  1 revision(s), 5 file(s) imported.

Verify

  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  5 files, 1 changesets, 5 total revisions

  $ hg manifest -r 0
  Main/#
  Main/%
  Main/*
  Main/@
  Main/a

  $ hg update tip
  5 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ ls Main
  #
  %
  *
  @
  a

End Test

  stopping the p4 server
