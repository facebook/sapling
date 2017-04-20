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
  $ p4d -f -J off >$P4ROOT/stdout 2>$P4ROOT/stderr &
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
  $ mkdir Main/b
  $ echo a > Main/a
  $ echo c > Main/b/c
  $ echo d > Main/d
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

  $ p4 delete Main/a
  //depot/Main/a#1 - opened for delete
  $ p4 submit -d second
  Submitting change 2.
  Locking 1 files ...
  delete //depot/Main/a#2
  Change 2 submitted.

  $ echo  a > Main/a
  $ p4 add Main/a
  //depot/Main/a#2 - opened for add
  $ p4 submit -d third
  Submitting change 3.
  Locking 1 files ...
  add //depot/Main/a#3
  Change 3 submitted.

Simple import

  $ cd $hgwd
  $ hg init --config 'format.usefncache=False'
  $ hg p4fastimport --debug -P $P4ROOT hg-p4-import
  loading changelist numbers.
  3 changelists to import.
  loading list of files.
  3 files to import.
  importing repository.
  writing filelog: b789fdd96dc2, p1 000000000000, linkrev 0, 2 bytes, src: *, path: depot/Main/a (glob)
  writing filelog: f9597ff22e3f, p1 b789fdd96dc2, linkrev 2, 2 bytes, src: *, path: depot/Main/a (glob)
  writing filelog: 149da44f2a4e, p1 000000000000, linkrev 0, 2 bytes, src: *, path: depot/Main/b/c (glob)
  writing filelog: a9092a3d84a3, p1 000000000000, linkrev 0, 2 bytes, src: *, path: depot/Main/d (glob)
  changelist 1: writing manifest. node: 17971aea5e86 p1: 000000000000 p2: 000000000000 linkrev: 0
  changelist 1: writing changelog: initial
  changelist 2: writing manifest. node: 6b6c47cf17c5 p1: 17971aea5e86 p2: 000000000000 linkrev: 1
  changelist 2: writing changelog: second
  changelist 3: writing manifest. node: ae40cd4cee98 p1: 6b6c47cf17c5 p2: 000000000000 linkrev: 2
  changelist 3: writing changelog: third
  3 revision(s), 3 file(s) imported.

Verify

  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  3 files, 3 changesets, 4 total revisions

  $ hg update tip
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

End Test

  stopping the p4 server
