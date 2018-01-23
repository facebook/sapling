#require p4

  $ . $TESTDIR/p4setup.sh

populate the depot
  $ mkdir Main
  $ mkdir Main/b
  $ echo a > Main/a
  $ ln -s ../a Main/b/c
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

  $ p4 edit Main/a Main/d
  //depot/Main/a#1 - opened for edit
  //depot/Main/d#1 - opened for edit
  $ echo a >> Main/a
  $ echo d >> Main/d
  $ p4 edit -ttext Main/b/c
  //depot/Main/b/c#1 - opened for edit
  $ rm Main/b/c
  $ echo d > Main/b/c
  $ p4 submit -d second
  Submitting change 2.
  Locking 3 files ...
  edit //depot/Main/a#2
  edit //depot/Main/b/c#2
  edit //depot/Main/d#2
  Change 2 submitted.
  $ echo "full-exec" > Main/full-exec
  $ p4 add -t text+Fx Main/full-exec
  //depot/Main/full-exec#1 - opened for add
  $ p4 submit -d third
  Submitting change 3.
  Locking 1 files ...
  add //depot/Main/full-exec#1
  Change 3 submitted.

Simple import

  $ cd $hgwd
  $ hg init --config 'format.usefncache=False'
  $ hg p4fastimport --debug -P $P4ROOT hg-p4-import
  loading changelist numbers.
  updating the branch cache (?)
  updating the branch cache (?)
  3 changelists to import.
  loading list of files.
  4 files to import.
  reading filelog * (glob)
  reading filelog * (glob)
  reading filelog * (glob)
  reading filelog * (glob)
  importing repository.
  writing filelog: b789fdd96dc2, p1 000000000000, linkrev 0, 2 bytes, src: *, path: Main/a (glob)
  writing filelog: a80d06849b33, p1 b789fdd96dc2, linkrev 1, 4 bytes, src: *, path: Main/a (glob)
  writing filelog: 1f6b5bb93f1d, p1 000000000000, linkrev 0, 4 bytes, src: *, path: Main/b/c (glob)
  writing filelog: c29ae1cbd245, p1 1f6b5bb93f1d, linkrev 1, 2 bytes, src: *, path: Main/b/c (glob)
  writing filelog: a9092a3d84a3, p1 000000000000, linkrev 0, 2 bytes, src: *, path: Main/d (glob)
  writing filelog: f83f0637e55e, p1 a9092a3d84a3, linkrev 1, 4 bytes, src: *, path: Main/d (glob)
  writing filelog: f011293652b8, p1 000000000000, linkrev 2, 10 bytes, src: *, path: Main/full-exec (glob)
  changelist 1: writing manifest. node: 05414d16d473 p1: 000000000000 p2: 000000000000 linkrev: 0
  changelist 1: writing changelog: initial
  changelist 2: writing manifest. node: 9408cdd6d4f7 p1: 05414d16d473 p2: 000000000000 linkrev: 1
  changelist 2: writing changelog: second
  changelist 3: writing manifest. node: c3be37ee7f64 p1: 9408cdd6d4f7 p2: 000000000000 linkrev: 2
  changelist 3: writing changelog: third
  updating the branch cache
  3 revision(s), 4 file(s) imported.

Verify

  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  4 files, 3 changesets, 7 total revisions

  $ hg update 0
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

# Ensure that the file is a symlink (-L) and is valid (-f) this ensures
# we correctly handle symlinks.

  $ test -L Main/b/c
  $ test -f Main/b/c
  $ hg --debug manifest -r 0
  b789fdd96dc2f3bd229c1dd8eedf0fc60e2b68e3 644   Main/a
  1f6b5bb93f1da278ef1fead1e4740a03d8802e9f 644 @ Main/b/c
  a9092a3d84a37b9993b5c73576f6de29b7ea50f6 755 * Main/d

  $ hg --debug manifest -r 1
  a80d06849b333b8a3d5c445f8ba3142010dcdc9e 644   Main/a
  c29ae1cbd245c01122ab671684e87b26183de12b 644   Main/b/c
  f83f0637e55e3c48e9922f14a016761626d79d3d 755 * Main/d

  $ hg update tip
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ test -x Main/full-exec

End Test

  stopping the p4 server
