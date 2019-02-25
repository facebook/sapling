#require p4

  $ . $TESTDIR/p4setup.sh

Populate depot
  $ mkdir foo
  $ mkdir bar
  $ echo foo > foo/foo.txt
  $ echo bar > bar/bar.txt
  $ p4 -q add foo/foo.txt bar/bar.txt
  $ p4 -q submit -d 'add foo/foo.txt and bar/bar.txt'
  $ p4 client -o | sed "s/$P4CLIENT/overlay-client/;/^View:/,$ d" > overlay
  $ cat >> overlay <<EOF
  > View:
  >   //depot/foo/... //overlay-client/...
  >   +//depot/bar/... //overlay-client/...
  > EOF
  $ p4 -q client -i < overlay

Setup hg repo
  $ cd $hgwd
  $ hg init --config 'format.usefncache=False'

Import!
  $ hg p4seqimport -P $P4ROOT -B master overlay-client
  $ hg manifest -r master
  bar.txt
  foo.txt

Move foo into bar
  $ p4 -q edit //depot/foo/...
  $ p4 -q move //depot/foo/... //depot/bar/...
  $ p4 submit -d 'move foo => bar @IMPORTER_IGNORE_REORG@'
  Submitting change 2.
  Locking 2 files ...
  move/add //depot/bar/foo.txt#1
  move/delete //depot/foo/foo.txt#2
  Change 2 submitted.

Import move, ensure ignore takes place
  $ hg p4seqimport --debug -P $P4ROOT -B master overlay-client
  incremental import from changelist: 2, node: * (glob)
  loading changelist numbers.
  1 changelists to import.
  importing CL2
  @IMPORTER_IGNORE_REORG@ found in CL desc, ignoring no-op moves
  Ignoring //depot/foo/foo.txt => //depot/bar/foo.txt, same path in hg: foo.txt
  committing changelog

Confirm there are no files in this commit and that changelist is correct
  $ hg log -r master -T '{files}'
  $ hg log -r master -T '{file_mods}'
  $ hg log -r master -T '{file_dels}'
  $ hg log -r master -T '{file_adds}'
  $ hg log -r master -T '{extras.p4changelist}\n'
  2

Move bar into foo
  $ p4 -q edit //depot/bar/...
  $ p4 -q move //depot/bar/... //depot/foo/...
  $ p4 submit -d 'move bar => foo'
  Submitting change 3.
  Locking 4 files ...
  move/delete //depot/bar/bar.txt#2
  move/delete //depot/bar/foo.txt#2
  move/add //depot/foo/bar.txt#1
  move/add //depot/foo/foo.txt#3
  Change 3 submitted.

Import - no ignore string, it will process like a normal changelist
and error because it will try to add/remove the same file
  $ hg p4seqimport --debug -P $P4ROOT -B master overlay-client
  incremental import from changelist: 3, node: * (glob)
  loading changelist numbers.
  1 changelists to import.
  importing CL3
  committing files:
  committing manifest
  committing changelog
  Failed importing CL3: bar.txt@*: not found in manifest (glob)
  transaction abort!
  rollback completed
  abort: bar.txt@*: not found in manifest! (glob)
  [255]

Update change to ignore moves and attempt import again
  $ p4 change -o 3 | sed 's: foo: foo @IMPORTER_IGNORE_REORG@:' > changeo3
  $ p4 change -fi < changeo3
  Change 3 updated.

  $ hg p4seqimport --debug -P $P4ROOT -B master overlay-client
  incremental import from changelist: 3, node: * (glob)
  loading changelist numbers.
  1 changelists to import.
  importing CL3
  @IMPORTER_IGNORE_REORG@ found in CL desc, ignoring no-op moves
  Ignoring //depot/bar/bar.txt => //depot/foo/bar.txt, same path in hg: bar.txt
  Ignoring //depot/bar/foo.txt => //depot/foo/foo.txt, same path in hg: foo.txt
  committing changelog

Confirm repository state is sane
  $ hg log -r master -T '{extras.p4changelist}\n'
  3
  $ hg cat -r master foo.txt
  foo
  $ hg cat -r master bar.txt
  bar
  $ hg debugindex foo.txt
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0       5     -1       0 2ed2a3912a0b 000000000000 000000000000
  $ hg debugindex bar.txt
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0       5     -1       0 b004912a8510 000000000000 000000000000

End Test
  stopping the p4 server
