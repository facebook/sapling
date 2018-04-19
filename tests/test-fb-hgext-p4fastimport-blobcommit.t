#require p4

  $ . $TESTDIR/p4setup.sh

  $ p4 client -o hg-p4-import-narrow | sed '/^View:/,$ d' >p4client
  $ echo "View:" >>p4client
  $ echo " //depot/Main/Narrow/... //hg-p4-import-narrow/Main/Narrow/..." >>p4client
  $ p4 client -i <p4client
  Client hg-p4-import-narrow saved.

Populate the Depot
  $ mkdir Main
  $ mkdir Main/Narrow
  $ echo a > Main/Narrow/a
  $ echo b > Main/Narrow/b
  $ ln -s a Main/Narrow/symlink
  $ p4 add Main/Narrow/a Main/Narrow/b Main/Narrow/symlink
  //depot/Main/Narrow/a#1 - opened for add
  //depot/Main/Narrow/b#1 - opened for add
  //depot/Main/Narrow/symlink#1 - opened for add
  $ p4 submit -d initial
  Submitting change 1.
  Locking 3 files ...
  add //depot/Main/Narrow/a#1
  add //depot/Main/Narrow/b#1
  add //depot/Main/Narrow/symlink#1
  Change 1 submitted.

  $ p4 edit Main/Narrow/a
  //depot/Main/Narrow/a#1 - opened for edit
  $ echo a >> Main/Narrow/a
  $ p4 edit -t text+x Main/Narrow/b
  //depot/Main/Narrow/b#1 - opened for edit
  $ p4 submit -d second
  Submitting change 2.
  Locking 2 files ...
  edit //depot/Main/Narrow/a#2
  edit //depot/Main/Narrow/b#2
  Change 2 submitted.

  $ mkdir Main/Outside
  $ echo a >> Main/Outside/a
  $ p4 add Main/Outside/a
  //depot/Main/Outside/a#1 - opened for add
  $ p4 submit -d third
  Submitting change 3.
  Locking 1 files ...
  add //depot/Main/Outside/a#1
  Change 3 submitted.

  $ p4 edit Main/Outside/a
  //depot/Main/Outside/a#1 - opened for edit
  $ echo a >> Main/Outside/a
  $ p4 submit -d fourth
  Submitting change 4.
  Locking 1 files ...
  edit //depot/Main/Outside/a#2
  Change 4 submitted.

Seq Import

  $ cd $hgwd
  $ hg init --config 'format.usefncache=False'
  $ hg p4seqimport --bookmark master -P $P4ROOT hg-p4-import-narrow

Verify

  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  3 files, 2 changesets, 4 total revisions

Sync Import

  $ hg p4syncimport --bookmark master --debug -P $P4ROOT hg-p4-import-narrow hg-p4-import
  incremental import from changelist: 3, node: * (glob)
  2 (current client) 4 (requested client) 2 (latest imported)
  latest change list number 4
  4 p4 filelogs to read
  1 new filelogs and 3 reuse filelogs
  running a sync import.
  writing filelog: b8a08782de62, p1 000000000000, linkrev 2, 4 bytes, src: *, path: Main/Outside/a (glob)
  changelist 4: writing manifest. node: * p1: ab2dd3ad9b8a p2: 000000000000 linkrev: 2 (glob)
  changelist 4: writing changelog: p4fastimport synchronizing client view
  writing bookmark
  updating the branch cache
  1 revision, 1 file(s) imported.

  $ hg manifest -r tip
  Main/Narrow/a
  Main/Narrow/b
  Main/Narrow/symlink
  Main/Outside/a

Verify

  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  4 files, 3 changesets, 5 total revisions

  $ hg update master
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark master)

Seq Import after Sync Import

  $ hg p4seqimport --bookmark master -P $P4ROOT hg-p4-import

Syncimport must abort if there are newer commits

  $ cd $p4wd
  $ p4 edit Main/Outside/a
  //depot/Main/Outside/a#2 - opened for edit
  $ echo a >> Main/Outside/a
  $ p4 submit -d fifth
  Submitting change 5.
  Locking 1 files ...
  edit //depot/Main/Outside/a#3
  Change 5 submitted.

  $ cd $hgwd
  $ hg p4syncimport --bookmark master --debug -P $P4ROOT hg-p4-import-narrow hg-p4-import
  incremental import from changelist: 5, node: * (glob)
  2 (current client) 5 (requested client) 4 (latest imported)
  abort: repository must contain most recent changes
  [255]

Remove stuff

  $ hg p4seqimport --bookmark master -P $P4ROOT hg-p4-import
  $ hg p4syncimport --bookmark master --debug -P $P4ROOT hg-p4-import hg-p4-import-narrow
  incremental import from changelist: 6, node: * (glob)
  5 (current client) 2 (requested client) 5 (latest imported)
  latest change list number 2
  3 p4 filelogs to read
  0 new filelogs and 3 reuse filelogs
  running a sync import.
  changelist 2: writing manifest. node: 75d827a98628 p1: 3a089c778dc1 p2: 000000000000 linkrev: 4
  changelist 2: writing changelog: p4fastimport synchronizing client view
  writing bookmark
  updating the branch cache
  1 revision, 0 file(s) imported.

  $ hg manifest -r tip
  Main/Narrow/a
  Main/Narrow/b
  Main/Narrow/symlink

End Test
  stopping the p4 server
