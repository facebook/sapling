#require p4

  $ . $TESTDIR/p4setup.sh
  $ cat >> $HGRCPATH<<EOF
  > [extensions]
  > lfs=$TESTDIR/../hgext3rd/lfs
  > [p4fastimport]
  > lfspointeronly=True
  > lfsmetadata=lfs.sql
  > [lfs]
  > threshold=10
  > EOF

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

Blob Commit

  $ cd $hgwd
  $ hg init --config 'format.usefncache=False'
  $ hg p4fastimport --debug -P $P4ROOT hg-p4-import
  loading changelist numbers.
  2 changelists to import.
  loading list of files.
  2 files to import.
  reading filelog * (glob)
  reading filelog * (glob)
  updating the branch cache (?)
  importing repository.
  writing filelog: b789fdd96dc2, p1 000000000000, linkrev 0, 2 bytes, src: *, path: Main/a (glob)
  writing filelog: a80d06849b33, p1 b789fdd96dc2, linkrev 1, 4 bytes, src: *, path: Main/a (glob)
  writing filelog: b3a729dd094e, p1 000000000000, linkrev 0, 44 bytes, src: *, path: Main/largefile (glob)
  largefile: Main/largefile, oid: 37a7b43abd9e105a0e6b22088b140735a02f288767fe7a6f4f436cb46b064ca9
  writing filelog: 9f14f96519e1, p1 b3a729dd094e, linkrev 1, 88 bytes, src: *, path: Main/largefile (glob)
  largefile: Main/largefile, oid: b0d5c1968efbabbff9d94160f284cd7b52686ca3c46cfffdd351de07384fce9c
  changelist 1: writing manifest. node: 9bbc5d2af2f4 p1: 000000000000 p2: 000000000000 linkrev: 0
  changelist 1: writing changelog: initial
  changelist 2: writing manifest. node: c14352bb3510 p1: 9bbc5d2af2f4 p2: 000000000000 linkrev: 1
  changelist 2: writing changelog: second
  writing lfs metadata to sqlite
  updating the branch cache
  2 revision(s), 2 file(s) imported.
  $ cd $p4wd
  $ echo thisisanotherlargefile > Main/anotherlargefile
  $ p4 add Main/anotherlargefile
  //depot/Main/anotherlargefile#1 - opened for add
  $ p4 submit -d third
  Submitting change 3.
  Locking 1 files ...
  add //depot/Main/anotherlargefile#1
  Change 3 submitted.
  $ cd $hgwd
  $ hg p4syncimport --debug -P $P4ROOT hg-p4-import
  incremental import from changelist: 3, node: * (glob)
  loading changelist numbers.
  1 changelists to import.
  loading list of files.
  1 files to import.
  reading filelog * (glob)
  running a sync import.
  writing filelog: cf38a89d2b54, p1 000000000000, linkrev 2, 23 bytes, src: *, path: Main/anotherlargefile (glob)
  largefile: Main/anotherlargefile, oid: 9703972eff7a4df07317eda436ab7ef827ed16ea28c62abdcd7de269745c610c
  changelist 3: writing manifest. node: edf0e5bc6eac p1: c14352bb3510 p2: 000000000000 linkrev: 2
  changelist 3: writing changelog: third
  writing lfs metadata to sqlite
  updating the branch cache
  1 revision(s), 1 file(s) imported.

Verify
(waiting for https://patchwork.mercurial-scm.org/patch/20582/)

  $ hg --debug verify --config verify.skipflags=8192
  repository uses revlog format 1
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  3 files, 3 changesets, 5 total revisions

  $ test -d .hg/store/lfs/objects
  [1]
  $ sqlite3 lfs.sql "SELECT * FROM p4_lfs_map"
  1|1|*|37a7b43abd9e105a0e6b22088b140735a02f288767fe7a6f4f436cb46b064ca9|//depot/Main/largefile (glob)
  2|2|*|b0d5c1968efbabbff9d94160f284cd7b52686ca3c46cfffdd351de07384fce9c|//depot/Main/largefile (glob)
  3|3|*|9703972eff7a4df07317eda436ab7ef827ed16ea28c62abdcd7de269745c610c|//depot/Main/anotherlargefile (glob)

End Test

  stopping the p4 server
