#require p4

  $ . $TESTDIR/p4setup.sh
  $ echo "[p4fastimport]" >> $HGRCPATH
  $ echo "useworker=force" >> $HGRCPATH

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

Simple import

  $ cd $hgwd
  $ hg init --config 'format.usefncache=False'
  $ FORCE_J=1 hg p4fastimport --debug -P $P4ROOT hg-p4-import
  loading changelist numbers.
  2 changelists to import.
  loading list of files.
  3 files to import.
  reading filelog * (glob)
  reading filelog * (glob)
  reading filelog * (glob)
  importing repository.
  writing filelog: * (glob)
  writing filelog: * (glob)
  writing filelog: * (glob)
  writing filelog: * (glob)
  writing filelog: * (glob)
  writing filelog: * (glob)
  changelist 1: writing manifest. node: a9f7e8df2a65 p1: 000000000000 p2: 000000000000 linkrev: 0
  changelist 1: writing changelog: initial
  changelist 2: writing manifest. node: e2b9d9177f8d p1: a9f7e8df2a65 p2: 000000000000 linkrev: 1
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

End Test

  stopping the p4 server
