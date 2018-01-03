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
  $ p4 edit Main/b/c
  //depot/Main/b/c#1 - opened for edit
  $ echo cc > Main/b/c
  $ p4 submit -d 'fourth'
  Submitting change 4.
  Locking 1 files ...
  edit //depot/Main/b/c#2
  Change 4 submitted.
  $ p4 files //depot/...
  //depot/Main/a#2 - edit change 2 (text)
  //depot/Main/b/c#2 - edit change 4 (text)
  //depot/Main/d#1 - add change 1 (text)
  //depot/Releases/0.1/a#1 - branch change 3 (text)
  //depot/Releases/0.1/b/c#1 - branch change 3 (text)
  //depot/Releases/0.1/d#1 - branch change 3 (text)
  $ p4 client -o | sed 's,//depot,//depot/Main,g' >p4client
  $ p4 client -i <p4client
  Client hg-p4-import saved.
  $ p4 client -o | sed 's,//depot/Main,//depot/Releases,g' | sed "s,$P4CLIENT,hg-p4-branch,g" >p4client-branch
  $ p4 client -i <p4client-branch
  Client hg-p4-branch saved.

Simple import

  $ cd $hgwd
  $ hg init --config 'format.usefncache=False'
  $ hg p4fastimport --bookmark master -P $P4ROOT hg-p4-import
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
  1 changelists to import.
  loading list of files.
  3 files to import.
  reading filelog * (glob)
  reading filelog * (glob)
  reading filelog * (glob)
  importing repository.
  writing filelog: a4bdc161c8fb, p1 000000000000, linkrev 3, 3 bytes, src: *, path: 0.1/a (glob)
  writing filelog: 149da44f2a4e, p1 000000000000, linkrev 3, 2 bytes, src: *, path: 0.1/b/c (glob)
  writing filelog: a9092a3d84a3, p1 000000000000, linkrev 3, 2 bytes, src: *, path: 0.1/d (glob)
  changelist 3: writing manifest. node: 1e17469076d6 p1: 3b31100ffd14 p2: 000000000000 linkrev: 3
  changelist 3: writing changelog: Populate //depot/Main/... //depot/Releases/0.1/....
  writing bookmark
  updating the branch cache
  1 revision(s), 3 file(s) imported.
  $ hg log --graph -T 'change: {rev}\nbookmark: {bookmarks}\ndesc: {desc|firstline}\n\n'
  o  change: 3
  |  bookmark: releases/test
  |  desc: Populate //depot/Main/... //depot/Releases/0.1/....
  |
  | o  change: 2
  |/   bookmark: master
  |    desc: fourth
  |
  o  change: 1
  |  bookmark:
  |  desc: second
  |
  o  change: 0
     bookmark:
     desc: first
  

Populate more commits to the branch

  $ cd $p4wd
  $ p4 -q --client hg-p4-branch sync
  $ p4 -q --client hg-p4-branch edit 0.1/a
  $ echo aaaa >> 0.1/a
  $ p4 -q --client hg-p4-branch submit -d'branch commit 1'
  $ p4 -q --client hg-p4-branch edit 0.1/a
  $ echo aaaa >> 0.1/a
  $ p4 -q --client hg-p4-branch submit -d'branch commit 2'

Test incremental import on branches

  $ cd $hgwd
  $ hg p4fastimport --bookmark releases/test --base 1 -P $P4ROOT hg-p4-branch
  $ hg log --graph -T 'change: {rev}\nbookmark: {bookmarks}\ndesc: {desc|firstline}\n\n'
  o  change: 5
  |  bookmark: releases/test
  |  desc: branch commit 2
  |
  o  change: 4
  |  bookmark:
  |  desc: branch commit 1
  |
  o  change: 3
  |  bookmark:
  |  desc: Populate //depot/Main/... //depot/Releases/0.1/....
  |
  | o  change: 2
  |/   bookmark: master
  |    desc: fourth
  |
  o  change: 1
  |  bookmark:
  |  desc: second
  |
  o  change: 0
     bookmark:
     desc: first
  

Verify

  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  6 files, 6 changesets, 10 total revisions

  $ hg update releases/test
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark releases/test)

  $ hg manifest -r releases/test
  0.1/a
  0.1/b/c
  0.1/d

  stopping the p4 server
