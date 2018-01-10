#require p4

  $ . $TESTDIR/p4setup.sh
  $ cat >> $HGRCPATH<<EOF
  > [extensions]
  > lfs=
  > [p4fastimport]
  > [lfs]
  > threshold=10
  > url=file:$TESTTMP/dummy-remote/
  > EOF

populate the depot
  $ mkdir Main
  $ mkdir Main/b
  $ echo a > Main/a
  $ echo c > Main/b/c
  $ echo thisisasuperlargefilebewithmorethank10Bsize >> Main/largefile
  $ p4 add Main/a Main/b/c Main/largefile
  //depot/Main/a#1 - opened for add
  //depot/Main/b/c#1 - opened for add
  //depot/Main/largefile#1 - opened for add
  $ p4 submit -d initial
  Submitting change 1.
  Locking 3 files ...
  add //depot/Main/a#1
  add //depot/Main/b/c#1
  add //depot/Main/largefile#1
  Change 1 submitted.

  $ p4 edit Main/a Main/b/c Main/largefile
  //depot/Main/a#1 - opened for edit
  //depot/Main/b/c#1 - opened for edit
  //depot/Main/largefile#1 - opened for edit
  $ echo a >> Main/a
  $ echo c >> Main/b/c
  $ echo thisisasuperlargefilebewithmorethank10Bsize >> Main/largefile
  $ p4 submit -d second
  Submitting change 2.
  Locking 3 files ...
  edit //depot/Main/a#2
  edit //depot/Main/b/c#2
  edit //depot/Main/largefile#2
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
  writing filelog: 149da44f2a4e, p1 000000000000, linkrev 0, 2 bytes, src: *, path: Main/b/c (glob)
  writing filelog: b11e10a88bfa, p1 149da44f2a4e, linkrev 1, 4 bytes, src: *, path: Main/b/c (glob)
  updating the branch cache (?)
  writing filelog: ee08366a1a83, p1 000000000000, linkrev 0, 44 bytes, src: rcs, path: Main/largefile
  largefile: Main/largefile, oid: dde0d1d11f099d6572fa47fcbd1cae324aeaad7409cb107461b09ba4eb2177ac
  writing filelog: a33f052256c3, p1 ee08366a1a83, linkrev 1, 88 bytes, src: rcs, path: Main/largefile
  largefile: Main/largefile, oid: 595efb640da040786d840fbae4675925fd4621f3498b849744ce0d4446674e3f
  changelist 1: writing manifest. node: e970866c1151 p1: 000000000000 p2: 000000000000 linkrev: 0
  changelist 1: writing changelog: initial
  changelist 2: writing manifest. node: 628c61b1c54a p1: e970866c1151 p2: 000000000000 linkrev: 1
  changelist 2: writing changelog: second
  updating the branch cache
  2 revision(s), 3 file(s) imported.

Verify
(waiting for https://patchwork.mercurial-scm.org/patch/20582/)

  $ hg --debug verify --config verify.skipflags=8192
  repository uses revlog format 1
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  3 files, 2 changesets, 6 total revisions

  $ test -d .hg/store/lfs/objects

# Ensure metadata is stored
  $ hg debugdata Main/largefile 0
  version https://git-lfs.github.com/spec/v1
  oid sha256:dde0d1d11f099d6572fa47fcbd1cae324aeaad7409cb107461b09ba4eb2177ac
  size 44
  x-is-binary 0

# Check the blobstore is populated
  $ find .hg/store/lfs/objects | sort
  .hg/store/lfs/objects
  .hg/store/lfs/objects/59
  .hg/store/lfs/objects/59/5efb640da040786d840fbae4675925fd4621f3498b849744ce0d4446674e3f
  .hg/store/lfs/objects/dd
  .hg/store/lfs/objects/dd/e0d1d11f099d6572fa47fcbd1cae324aeaad7409cb107461b09ba4eb2177ac

# Check the blob stored contains the actual contents of the file
  $ cat .hg/store/lfs/objects/59/5efb640da040786d840fbae4675925fd4621f3498b849744ce0d4446674e3f
  thisisasuperlargefilebewithmorethank10Bsize
  thisisasuperlargefilebewithmorethank10Bsize

End Test

  stopping the p4 server
