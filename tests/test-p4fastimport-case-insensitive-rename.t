#require p4

  $ PYTHONPATH=$TESTDIR/..:$PYTHONPATH
  $ export PYTHONPATH

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "p4fastimport= " >> $HGRCPATH

create p4 depot
  $ p4wd=`pwd`/p4
  $ hgwd=`pwd`/hg
  $ P4ROOT=`pwd`/depot; export P4ROOT
  $ P4AUDIT=$P4ROOT/audit; export P4AUDIT
  $ P4JOURNAL=$P4ROOT/journal; export P4JOURNAL
  $ P4LOG=$P4ROOT/log; export P4LOG
  $ P4PORT=localhost:$HGPORT; export P4PORT
  $ P4DEBUG=1; export P4DEBUG

  $ mkdir $hgwd
  $ mkdir $p4wd
  $ cd $p4wd

start the p4 server
  $ [ ! -d $P4ROOT ] && mkdir $P4ROOT
  $ p4d -C1 -f -J off >$P4ROOT/stdout 2>$P4ROOT/stderr &
  $ echo $! >> $DAEMON_PIDS
  $ trap "echo stopping the p4 server ; p4 admin stop" EXIT

  $ # wait for the server to initialize
  $ while ! p4 ; do
  >    sleep 1
  > done >/dev/null 2>/dev/null

create a client spec
  $ cd $p4wd
  $ P4CLIENT=hg-p4-import; export P4CLIENT
  $ DEPOTPATH=//depot/...
  $ p4 client -o | sed '/^View:/,$ d' >p4client
  $ echo View: >>p4client
  $ echo " $DEPOTPATH //$P4CLIENT/..." >>p4client
  $ p4 client -i <p4client
  Client hg-p4-import saved.

populate the depot
  $ mkdir Main
  $ echo a > Main/a
  $ p4 add Main/a
  //depot/Main/a#1 - opened for add
  $ p4 submit -d initial
  Submitting change 1.
  Locking 1 files ...
  add //depot/Main/a#1
  Change 1 submitted.
  $ p4 edit Main/a
  //depot/Main/a#1 - opened for edit
  $ p4 move Main/a Main/b
  //depot/Main/b#1 - moved from //depot/Main/a#1
  $ p4 submit -d moveway
  Submitting change 2.
  Locking 2 files ...
  move/delete //depot/Main/a#2
  move/add //depot/Main/b#1
  Change 2 submitted.
  $ p4 edit Main/b
  //depot/Main/b#1 - opened for edit
  $ p4 move Main/b Main/A
  //depot/Main/A#2 - moved from //depot/Main/b#1
  $ p4 submit -d moveback
  Submitting change 3.
  Locking 2 files ...
  move/add //depot/Main/A#3
  move/delete //depot/Main/b#2
  Change 3 submitted.

import

  $ cd $hgwd
  $ hg init --config 'format.usefncache=False'
  $ hg p4fastimport --debug -P $P4ROOT hg-p4-import
  loading changelist numbers.
  3 changelists to import.
  loading list of files.
  3 files to import.
  reading filelog * (glob)
  reading filelog * (glob)
  reading filelog * (glob)
  importing repository.
  case conflict: //depot/Main/A and //depot/Main/a
  writing filelog: b789fdd96dc2, p1 000000000000, linkrev 2, 2 bytes, src: *, path: Main/A (glob)
  writing filelog: b789fdd96dc2, p1 000000000000, linkrev 0, 2 bytes, src: *, path: Main/a (glob)
  writing filelog: b789fdd96dc2, p1 000000000000, linkrev 1, 2 bytes, src: *, path: Main/b (glob)
  changelist 1: writing manifest. node: f495e209f723 p1: 000000000000 p2: 000000000000 linkrev: 0
  changelist 1: writing changelog: initial
  changelist 2: writing manifest. node: 510da33a44e3 p1: f495e209f723 p2: 000000000000 linkrev: 1
  changelist 2: writing changelog: moveway
  changelist 3: writing manifest. node: 6541d210de72 p1: 510da33a44e3 p2: 000000000000 linkrev: 2
  changelist 3: writing changelog: moveback
  3 revision(s), 3 file(s) imported.

Verify

  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  3 files, 3 changesets, 3 total revisions

Update

  $ hg manifest -r 0
  Main/a
  $ hg manifest -r 1
  Main/b
  $ hg manifest -r 2
  Main/A
  $ hg update -r 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat Main/a
  a
  $ hg update -r 1
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ cat Main/b
  a
  $ hg update -r 2
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ cat Main/A
  a
  stopping the p4 server
