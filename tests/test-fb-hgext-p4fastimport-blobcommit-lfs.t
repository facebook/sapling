  $ setconfig extensions.treemanifest=!
#require p4

  $ . $TESTDIR/p4setup.sh
  $ cat >> $HGRCPATH<<EOF
  > [extensions]
  > lfs=
  > [p4fastimport]
  > lfspointeronly=True
  > lfsmetadata=lfs.sql
  > [lfs]
  > threshold=10
  > EOF

  $ p4 client -o hg-p4-import-narrow | sed '/^View:/,$ d' >p4client
  $ echo "View:" >>p4client
  $ echo " //depot/Main/... //hg-p4-import-narrow/Main/..." >>p4client
  $ p4 client -i <p4client
  Client hg-p4-import-narrow saved.

populate the depot
  $ mkdir Main
  $ echo a > Main/a
  $ echo thisisasuperlargefilebewithmorethank10ksize >> Main/largefile
  $ p4 add Main/a  Main/largefile
  //depot/Main/a#1 - opened for add
  //depot/Main/largefile#1 - opened for add
  $ p4 submit -d initial
  Submitting change 1.
  Locking 2 files ...
  add //depot/Main/a#1
  add //depot/Main/largefile#1
  Change 1 submitted.

  $ p4 edit Main/a  Main/largefile
  //depot/Main/a#1 - opened for edit
  //depot/Main/largefile#1 - opened for edit
  $ echo a >> Main/a
  $ echo thisisasuperlargefilebewithmorethank10ksize >> Main/largefile
  $ p4 submit -d second
  Submitting change 2.
  Locking 2 files ...
  edit //depot/Main/a#2
  edit //depot/Main/largefile#2
  Change 2 submitted.

Sync Commit

  $ cd $hgwd
  $ hg init --config 'format.usefncache=False'
  $ hg p4seqimport --debug -P $P4ROOT hg-p4-import-narrow
  loading changelist numbers.
  2 changelists to import.
  importing CL1
  committing files:
  Main/a
  file: //depot/Main/a, src: * (glob)
  Main/largefile
  file: //depot/Main/largefile, src: * (glob)
  committing manifest
  committing changelog
  largefile: Main/largefile, oid: 37a7b43abd9e105a0e6b22088b140735a02f288767fe7a6f4f436cb46b064ca9
  writing lfs metadata to sqlite
  importing CL2
  file: //depot/Main/a, src: * (glob)
  file: //depot/Main/largefile, src: * (glob)
  committing files:
  Main/a
  Main/largefile
  committing manifest
  committing changelog
  largefile: Main/largefile, oid: b0d5c1968efbabbff9d94160f284cd7b52686ca3c46cfffdd351de07384fce9c
  writing lfs metadata to sqlite


  $ cd $p4wd
  $ mkdir Outside
  $ echo b > Outside/b
  $ echo thisisanotherlargefile > Outside/anotherlargefile
  $ p4 add Outside/b Outside/anotherlargefile
  //depot/Outside/b#1 - opened for add
  //depot/Outside/anotherlargefile#1 - opened for add
  $ p4 submit -d third
  Submitting change 3.
  Locking 2 files ...
  add //depot/Outside/anotherlargefile#1
  add //depot/Outside/b#1
  Change 3 submitted.

  $ cd $hgwd
  $ hg p4syncimport --debug -P $P4ROOT hg-p4-import-narrow hg-p4-import
  incremental import from changelist: 3, node: * (glob)
  2 (current client) 3 (requested client) 2 (latest imported)
  latest change list number 3
  2 added files
  0 removed files
  committing files:
  Outside/anotherlargefile
  file: //depot/Outside/anotherlargefile, src: * (glob)
  Outside/b
  file: //depot/Outside/b, src: * (glob)
  committing manifest
  committing changelog
  largefile: Outside/anotherlargefile, oid: 9703972eff7a4df07317eda436ab7ef827ed16ea28c62abdcd7de269745c610c
  writing lfs metadata to sqlite

  $ hg manifest -vr tip
  644   Main/a
  644   Main/largefile
  644   Outside/anotherlargefile
  644   Outside/b

Verify
(waiting for https://patchwork.mercurial-scm.org/patch/20582/)

  $ cd $hgwd
  $ hg --debug verify --config verify.skipflags=8192
  repository uses revlog format 1
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  4 files, 3 changesets, 6 total revisions

  $ test -d .hg/store/lfs/objects
  $ sqlite3 lfs.sql "SELECT * FROM p4_lfs_map"
  1|1|*|37a7b43abd9e105a0e6b22088b140735a02f288767fe7a6f4f436cb46b064ca9|//depot/Main/largefile (glob)
  2|2|*|b0d5c1968efbabbff9d94160f284cd7b52686ca3c46cfffdd351de07384fce9c|//depot/Main/largefile (glob)
  3|3|*|9703972eff7a4df07317eda436ab7ef827ed16ea28c62abdcd7de269745c610c|//depot/Outside/anotherlargefile (glob)

End Test

  stopping the p4 server
