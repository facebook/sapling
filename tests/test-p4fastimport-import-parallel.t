#require p4

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "p4fastimport=" >> $HGRCPATH
  $ echo "[p4fastimport]" >> $HGRCPATH
  $ echo "useworker=force" >> $HGRCPATH

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

  $ p4 edit Main/a Main/b/c Main/d
  //depot/Main/a#1 - opened for edit
  //depot/Main/b/c#1 - opened for edit
  //depot/Main/d#1 - opened for edit
  $ echo a >> Main/a
  $ echo c >> Main/b/c
  $ echo d >> Main/d
  $ p4 submit -d second
  Submitting change 2.
  Locking 3 files ...
  edit //depot/Main/a#2
  edit //depot/Main/b/c#2
  edit //depot/Main/d#2
  Change 2 submitted.

Simple import

  $ cd $hgwd
  $ hg init --config 'format.usefncache=False'
  $ FORCE_J=1 hg p4fastimport --debug -P $P4ROOT hg-p4-import
  loading changelist numbers.
  2 changelists to import.
  loading list of files.
  3 files to import.
  importing repository.
  writing filelog: * (glob)
  writing filelog: * (glob)
  writing filelog: * (glob)
  writing filelog: * (glob)
  writing filelog: * (glob)
  writing filelog: * (glob)
  changelist 1: writing manifest. node: 17971aea5e86 p1: 000000000000 p2: 000000000000 linkrev: 0
  changelist 1: writing changelog: initial
  changelist 2: writing manifest. node: ee5866cdbad7 p1: 17971aea5e86 p2: 000000000000 linkrev: 1
  changelist 2: writing changelog: second
  2 revision(s), 3 file(s) imported.

Verify

  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  3 files, 2 changesets, 6 total revisions

  $ hg update tip
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

End Test

  stopping the p4 server
