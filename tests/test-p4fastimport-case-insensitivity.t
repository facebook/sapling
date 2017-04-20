#require p4

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
  $ p4 delete Main/a
  //depot/Main/a#1 - opened for delete
  $ p4 submit -ddelete
  Submitting change 2.
  Locking 1 files ...
  delete //depot/Main/a#2
  Change 2 submitted.
  $ echo a > Main/A
  $ p4 add Main/A
  //depot/Main/A#2 - opened for add
  $ p4 submit -d 'add with case-inensitivity match'
  Submitting change 3.
  Locking 1 files ...
  add //depot/Main/A#3
  Change 3 submitted.

import

  $ cd $hgwd
  $ hg init --config 'format.usefncache=False'
  $ hg p4fastimport --debug -P $P4ROOT hg-p4-import
  loading changelist numbers.
  3 changelists to import.
  loading list of files.
  2 files to import.
  importing repository.
  case conflict: //depot/Main/A and //depot/Main/a
  writing filelog: b789fdd96dc2, p1 000000000000, linkrev 2, 2 bytes, src: *, path: depot/Main/A (glob)
  writing filelog: b789fdd96dc2, p1 000000000000, linkrev 0, 2 bytes, src: *, path: depot/Main/a (glob)
  changelist 1: writing manifest. node: 77111a2fe360 p1: 000000000000 p2: 000000000000 linkrev: 0
  changelist 1: writing changelog: initial
  changelist 2: writing manifest. node: ba644731a088 p1: 77111a2fe360 p2: 000000000000 linkrev: 1
  changelist 2: writing changelog: delete
  changelist 3: writing manifest. node: a7bdbbc64a41 p1: ba644731a088 p2: 000000000000 linkrev: 2
  changelist 3: writing changelog: add with case-inensitivity match
  3 revision(s), 2 file(s) imported.

Verify

  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  2 files, 3 changesets, 2 total revisions

Update

  $ hg update -r 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat depot/Main/a
  a
  $ hg update -r 1
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg update -r 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat depot/Main/A
  a
  stopping the p4 server
