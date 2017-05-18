#require p4

  $ . $TESTDIR/p4setup.sh

populate the depot
  $ mkdir Main
  $ mkdir Main/b
  $ echo a > Main/a
  $ echo c > Main/b/c
  $ echo d > Main/d
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
  $ echo c >> Main/b/c
  $ echo d >> Main/d
  $ p4 submit -d second
  Submitting change 2.
  Locking 3 files ...
  edit //depot/Main/a#2
  edit //depot/Main/b/c#2
  edit //depot/Main/d#2
  Change 2 submitted.

Test archiving something

  $ cat >desc <<EOF
  > Depot: archive
  > Description: Archive
  > Type: archive
  > Map: archive/...
  > EOF
  $ p4 depot -i <desc
  Depot archive saved.
  $ p4 archive -t -D archive //depot/Main/d
  Archiving //depot/Main/d#2 to //archive/depot/Main/d.
  Archiving //depot/Main/d#1 to //archive/depot/Main/d.

Simple import

  $ cd $hgwd
  $ hg init --config 'format.usefncache=False'
  $ hg p4fastimport --bookmark master --debug -P $P4ROOT hg-p4-import
  loading changelist numbers.
  2 changelists to import.
  loading list of files.
  2 files to import.
  reading filelog * (glob)
  reading filelog * (glob)
  importing repository.
  writing filelog: b789fdd96dc2, p1 000000000000, linkrev 0, 2 bytes, src: *, path: Main/a (glob)
  writing filelog: a80d06849b33, p1 b789fdd96dc2, linkrev 1, 4 bytes, src: *, path: Main/a (glob)
  writing filelog: 149da44f2a4e, p1 000000000000, linkrev 0, 2 bytes, src: *, path: Main/b/c (glob)
  writing filelog: b11e10a88bfa, p1 149da44f2a4e, linkrev 1, 4 bytes, src: *, path: Main/b/c (glob)
  changelist 1: writing manifest. node: a9ab65129a6d p1: 000000000000 p2: 000000000000 linkrev: 0
  changelist 1: writing changelog: initial
  changelist 2: writing manifest. node: aff99eae550e p1: a9ab65129a6d p2: 000000000000 linkrev: 1
  changelist 2: writing changelog: second
  writing bookmark
  updating the branch cache (?)
  2 revision(s), 2 file(s) imported.

Verify

  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  2 files, 2 changesets, 4 total revisions

  $ hg update master
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark master)

  $ hg manifest -r master
  Main/a
  Main/b/c

End Test

  stopping the p4 server
