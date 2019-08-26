#require p4

  $ . $TESTDIR/p4setup.sh

Populate depot
  $ mkdir foo
  $ mkdir bar
  $ echo a > foo/a
  $ echo b > bar/b
  $ p4 add foo/a bar/b
  //depot/foo/a#1 - opened for add
  //depot/bar/b#1 - opened for add
  $ p4 submit -d 'add foo/a and bar/b'
  Submitting change 1.
  Locking 2 files ...
  add //depot/bar/b#1
  add //depot/foo/a#1
  Change 1 submitted.
  $ p4 client -o | sed "s:depot:depot/foo:g;s:$P4CLIENT:foo:g" > foo-client
  $ p4 client -i <foo-client
  Client foo saved.

  $ p4 edit bar/b
  //depot/bar/b#1 - opened for edit
  $ echo bb >> bar/b
  $ p4 submit -d 'edit bar/b'
  Submitting change 2.
  Locking 1 files ...
  edit //depot/bar/b#2
  Change 2 submitted.

  $ p4 edit bar/b foo/a
  //depot/bar/b#2 - opened for edit
  //depot/foo/a#1 - opened for edit
  $ p4 move bar/b foo/b
  //depot/foo/b#1 - moved from //depot/bar/b#2
  $ p4 move foo/a bar/a
  //depot/bar/a#1 - moved from //depot/foo/a#1
  $ p4 submit -d 'move bar/b to foo/b and foo/a to bar/a'
  Submitting change 3.
  Locking 4 files ...
  move/add //depot/bar/a#1
  move/delete //depot/bar/b#3
  move/delete //depot/foo/a#2
  move/add //depot/foo/b#1
  Change 3 submitted.

Setup hg repo
  $ cd $hgwd
  $ hg init --config 'format.usefncache=False'

Import!
Confirms changelist 2 is ignored (it only touches bar which is not part of foo)
Assert changelist 3 is imported as adding foo/b only (bar is not part of foo so
it cannot be used as source for move operation)
  $ hg p4seqimport --debug -B master -P $P4ROOT foo
  loading changelist numbers.
  2 changelists to import.
  importing CL1
  file: //depot/foo/a, src: * (glob)
  committing files:
  a
  committing manifest
  committing changelog
  importing CL3
  file: //depot/foo/b, src: * (glob)
  committing files:
  b
  committing manifest
  committing changelog

  $ hg log -r master -T 'A:\n{file_adds}\nD:\n{file_dels}\nM:{file_copies}\n'
  A:
  b
  D:
  a
  M:

End Test
  stopping the p4 server
