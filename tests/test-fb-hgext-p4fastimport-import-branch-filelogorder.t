#require p4

  $ . $TESTDIR/p4setup.sh

Populate the depot

  $ mkdir Main
  $ mkdir Main/b
  $ echo a > Main/a
  $ echo c > Main/b/c
  $ echo d > Main/d
  $ p4 add Main/a Main/b/c Main/d
  //depot/Main/a#1 - opened for add
  //depot/Main/b/c#1 - opened for add
  //depot/Main/d#1 - opened for add
  $ p4 submit -d 'first'
  Submitting change 1.
  Locking 3 files ...
  add //depot/Main/a#1
  add //depot/Main/b/c#1
  add //depot/Main/d#1
  Change 1 submitted.
  $ p4 edit Main/a
  //depot/Main/a#1 - opened for edit
  $ echo aa > Main/a
  $ p4 submit -d 'second'
  Submitting change 2.
  Locking 1 files ...
  edit //depot/Main/a#2
  Change 2 submitted.
  $ p4 populate //depot/Main/... //depot/Releases/0.1/...
  3 files branched (change 3).
  $ p4 edit Main/b/c Main/a
  //depot/Main/b/c#1 - opened for edit
  //depot/Main/a#2 - opened for edit
  $ echo cc > Main/b/c
  $ echo aa >> Main/a
  $ p4 submit -d 'fourth'
  Submitting change 4.
  Locking 2 files ...
  edit //depot/Main/a#3
  edit //depot/Main/b/c#2
  Change 4 submitted.
  $ p4 files //depot/...
  //depot/Main/a#3 - edit change 4 (text)
  //depot/Main/b/c#2 - edit change 4 (text)
  //depot/Main/d#1 - add change 1 (text)
  //depot/Releases/0.1/a#1 - branch change 3 (text)
  //depot/Releases/0.1/b/c#1 - branch change 3 (text)
  //depot/Releases/0.1/d#1 - branch change 3 (text)
  $ p4 client -o | sed 's,//depot,//depot/Main,g' >p4client
  $ p4 client -i <p4client
  Client hg-p4-import saved.
  $ p4 client -o | sed 's,//depot/Main,//depot/Releases/0.1,g' | sed "s,$P4CLIENT,hg-p4-branch,g" >p4client-branch
  $ p4 client -i <p4client-branch
  Client hg-p4-branch saved.
  $ p4 -q --client hg-p4-branch sync
  $ p4 --client hg-p4-branch edit a
  //depot/Releases/0.1/a#1 - opened for edit
  $ echo aaa >> a
  $ p4 --client hg-p4-branch submit -d'fifth'
  Submitting change 5.
  Locking 1 files ...
  edit //depot/Releases/0.1/a#2
  Change 5 submitted.

Simple import

  $ cd $hgwd
  $ hg init --config 'format.usefncache=False'
  $ hg p4fastimport --debug --bookmark master -P $P4ROOT hg-p4-import
  loading changelist numbers.
  3 changelists to import.
  loading list of files.
  3 files to import.
  reading filelog * (glob)
  reading filelog * (glob)
  reading filelog * (glob)
  importing repository.
  writing filelog: b789fdd96dc2, p1 000000000000, linkrev 0, 2 bytes, src: *, path: a (glob)
  writing filelog: bc1909078145, p1 b789fdd96dc2, linkrev 1, 3 bytes, src: *, path: a (glob)
  writing filelog: a3e0cc7b33db, p1 bc1909078145, linkrev 2, 6 bytes, src: *, path: a (glob)
  writing filelog: 149da44f2a4e, p1 000000000000, linkrev 0, 2 bytes, src: *, path: b/c (glob)
  writing filelog: 1f38a5c14c2d, p1 149da44f2a4e, linkrev 2, 3 bytes, src: *, path: b/c (glob)
  writing filelog: a9092a3d84a3, p1 000000000000, linkrev 0, 2 bytes, src: *, path: d (glob)
  changelist 1: writing manifest. node: 096669767a87 p1: 000000000000 p2: 000000000000 linkrev: 0
  changelist 1: writing changelog: first
  changelist 2: writing manifest. node: 3b31100ffd14 p1: 096669767a87 p2: 000000000000 linkrev: 1
  changelist 2: writing changelog: second
  changelist 4: writing manifest. node: 9be416d40e06 p1: 3b31100ffd14 p2: 000000000000 linkrev: 2
  changelist 4: writing changelog: fourth
  writing bookmark
  updating the branch cache
  3 revision(s), 3 file(s) imported.

  $ hg log --graph -T 'change: {rev}\nbookmark: {bookmarks}\ndesc: {desc|firstline}\n\n'
  o  change: 2
  |  bookmark: master
  |  desc: fourth
  |
  o  change: 1
  |  bookmark:
  |  desc: second
  |
  o  change: 0
     bookmark:
     desc: first
  

Test branch import

  $ hg p4fastimport --debug --bookmark releases/test --base 1 -P $P4ROOT hg-p4-branch
  incremental import from changelist: 3, node: * (glob)
  creating branchpoint, base * (glob)
  loading changelist numbers.
  2 changelists to import.
  loading list of files.
  3 files to import.
  reading filelog * (glob)
  reading filelog * (glob)
  reading filelog * (glob)
  importing repository.
  writing filelog: 55e0c41b3b9d, p1 bc1909078145, linkrev 3, 3 bytes, src: *, path: a (glob)
  writing filelog: 0d757cbefdd9, p1 55e0c41b3b9d, linkrev 4, 7 bytes, src: *, path: a (glob)
  writing filelog: 825765709d02, p1 149da44f2a4e, linkrev 3, 2 bytes, src: *, path: b/c (glob)
  writing filelog: a8afb6cbe637, p1 a9092a3d84a3, linkrev 3, 2 bytes, src: *, path: d (glob)
  changelist 3: writing manifest. node: d1db45c7804a p1: 3b31100ffd14 p2: 000000000000 linkrev: 3
  changelist 3: writing changelog: Populate //depot/Main/... //depot/Releases/0.1/....
  changelist 5: writing manifest. node: 2c67bace730f p1: d1db45c7804a p2: 000000000000 linkrev: 4
  changelist 5: writing changelog: fifth
  writing bookmark
  updating the branch cache
  2 revision(s), 3 file(s) imported.
  $ hg log --graph -T 'change: {rev}\nbookmark: {bookmarks}\ndesc: {desc|firstline}\nfiles: {files}\n\n'
  o  change: 4
  |  bookmark: releases/test
  |  desc: fifth
  |  files: a
  |
  o  change: 3
  |  bookmark:
  |  desc: Populate //depot/Main/... //depot/Releases/0.1/....
  |  files: a b/c d
  |
  | o  change: 2
  |/   bookmark: master
  |    desc: fourth
  |    files: a b/c
  |
  o  change: 1
  |  bookmark:
  |  desc: second
  |  files: a
  |
  o  change: 0
     bookmark:
     desc: first
     files: a b/c d
  

  $ hg debugindex a
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0       3     -1       0 b789fdd96dc2 000000000000 000000000000
       1         3       4     -1       1 bc1909078145 b789fdd96dc2 000000000000
       2         7       7     -1       2 a3e0cc7b33db bc1909078145 000000000000
       3        14       0      1       3 55e0c41b3b9d bc1909078145 000000000000
       4        14       8     -1       4 0d757cbefdd9 55e0c41b3b9d 000000000000


  stopping the p4 server
