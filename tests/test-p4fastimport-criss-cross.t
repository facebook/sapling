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
  $ echo '1' > Main/a
  $ p4 add Main/a
  //depot/Main/a#1 - opened for add
  $ p4 submit -d'CL1(1)'
  Submitting change 1.
  Locking 1 files ...
  add //depot/Main/a#1
  Change 1 submitted.

  $ p4 edit Main/a
  //depot/Main/a#1 - opened for edit
  $ echo '4' > Main/a
  $ cat >desc <<EOF
  > Change: new
  > Client: $P4CLIENT
  > User: $USER
  > Status: new
  > Description: CL4(2)
  > Files:
  >     //depot/Main/a # edit
  > EOF
  $ p4 shelve -i < desc
  Change 2 created with 1 open file(s).
  Shelving files for change 2.
  edit //depot/Main/a#1
  Change 2 files shelved.
  $ p4 reopen -c default Main/a
  //depot/Main/a#1 - reopened; default change
  $ echo '3' > Main/a
  $ p4 submit -d'CL3(3)'
  Submitting change 3.
  Locking 1 files ...
  edit //depot/Main/a#2
  Change 3 submitted.
  $ cat Main/a
  3
  $ p4 unshelve -s 2 -c 2
  //depot/Main/a#1 - unshelved, opened for edit
  $ p4 shelve -c 2 -d
  Shelved change 2 deleted.
  $ p4 sync
  //depot/Main/a#2 - is opened and not being changed
  * //depot/Main/a - must resolve #2 before submitting (glob)
  $ p4 resolve -ay
  $TESTTMP/p4/Main/a - vs //depot/Main/a#2
  //hg-p4-import/Main/a - ignored //depot/Main/a
  $ p4 submit -c 2
  Submitting change 2.
  Locking 1 files ...
  edit //depot/Main/a#3
  Change 2 renamed change 4 and submitted.
  $ cat Main/a
  4

Import

  $ cd $hgwd
  $ hg init --config 'format.usefncache=False'
  $ hg p4fastimport --debug -P $P4ROOT hg-p4-import
  loading changelist numbers.
  3 changelists to import.
  loading list of files.
  1 files to import.
  importing repository.
  writing filelog: b8e02f643373, p1 000000000000, linkrev 0, 2 bytes, src: *, path: depot/Main/a (glob)
  writing filelog: 059c099e8c05, p1 b8e02f643373, linkrev 1, 2 bytes, src: *, path: depot/Main/a (glob)
  writing filelog: de9e19b2b7a1, p1 059c099e8c05, linkrev 2, 2 bytes, src: *, path: depot/Main/a (glob)
  changelist 1: Writing manifest.
  changelist 1: Writing changelog: CL1(1)
  changelist 3: Writing manifest.
  changelist 3: Writing changelog: CL3(3)
  changelist 4: Writing manifest.
  changelist 4: Writing changelog: CL4(2)
  3 revision(s), 1 file(s) imported.
  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 3 changesets, 3 total revisions
  $ hg cat -r tip depot/Main/a
  4
  stopping the p4 server
