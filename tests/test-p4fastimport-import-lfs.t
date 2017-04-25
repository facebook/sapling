#require p4

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "p4fastimport= " >> $HGRCPATH
  $ echo "lfs=" >> $HGRCPATH
  $ echo "[p4fastimport]" >> $HGRCPATH
  $ echo "lfspointeronly=True" >> $HGRCPATH
  $ echo "lfsmetadata=lfs.sql" >> $HGRCPATH
  $ echo "[lfs]" >> $HGRCPATH
  $ echo "threshold=10" >> $HGRCPATH
  $ echo "remoteurl=https://dewey-lfs.vip.facebook.com/lfs" >> $HGRCPATH

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
  $ echo thisisasuperlargefilebewithmorethank10ksize >> Main/largefile
  $ p4 add Main/a Main/b/c Main/largefile
  //depot/Main/a#1 - opened for add
  //depot/Main/b/c#1 - opened for add
  //depot/Main/largefile#1 - opened for add
  $ p4 submit -d initial
  Submitting change 1.
  Locking 3 files ...
  add //depot/Main/a#1
  add //depot/Main/b/c#1
  add //depot/Main/largefile#1
  Change 1 submitted.

  $ p4 edit Main/a Main/b/c Main/largefile
  //depot/Main/a#1 - opened for edit
  //depot/Main/b/c#1 - opened for edit
  //depot/Main/largefile#1 - opened for edit
  $ echo a >> Main/a
  $ echo c >> Main/b/c
  $ echo thisisasuperlargefilebewithmorethank10ksize >> Main/largefile
  $ p4 submit -d second
  Submitting change 2.
  Locking 3 files ...
  edit //depot/Main/a#2
  edit //depot/Main/b/c#2
  edit //depot/Main/largefile#2
  Change 2 submitted.

Simple import

  $ cd $hgwd
  $ hg init --config 'format.usefncache=False'
  $ hg p4fastimport --debug -P $P4ROOT hg-p4-import
  loading changelist numbers.
  2 changelists to import.
  loading list of files.
  3 files to import.
  importing repository.
  writing filelog: b789fdd96dc2, p1 000000000000, linkrev 0, 2 bytes, src: *, path: Main/a (glob)
  writing filelog: a80d06849b33, p1 b789fdd96dc2, linkrev 1, 4 bytes, src: *, path: Main/a (glob)
  writing filelog: 149da44f2a4e, p1 000000000000, linkrev 0, 2 bytes, src: *, path: Main/b/c (glob)
  writing filelog: b11e10a88bfa, p1 149da44f2a4e, linkrev 1, 4 bytes, src: *, path: Main/b/c (glob)
  writing filelog: b3a729dd094e, p1 000000000000, linkrev 0, 44 bytes, src: *, path: Main/largefile (glob)
  largefile: Main/largefile, oid: 37a7b43abd9e105a0e6b22088b140735a02f288767fe7a6f4f436cb46b064ca9
  writing filelog: 9f14f96519e1, p1 b3a729dd094e, linkrev 1, 88 bytes, src: *, path: Main/largefile (glob)
  largefile: Main/largefile, oid: b0d5c1968efbabbff9d94160f284cd7b52686ca3c46cfffdd351de07384fce9c
  changelist 1: writing manifest. node: 0637b0361958 p1: 000000000000 p2: 000000000000 linkrev: 0
  changelist 1: writing changelog: initial
  changelist 2: writing manifest. node: 31c95d82cc49 p1: 0637b0361958 p2: 000000000000 linkrev: 1
  changelist 2: writing changelog: second
  writing lfs metadata to sqlite
  2 revision(s), 3 file(s) imported.

Verify

  $ hg --debug verify
  repository uses revlog format 1
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  3 files, 2 changesets, 6 total revisions

  $ test -d .hg/cache/localblobstore
  [1]
  $ sqlite3 lfs.sql "SELECT * FROM p4_lfs_map"
  1|1|*|37a7b43abd9e105a0e6b22088b140735a02f288767fe7a6f4f436cb46b064ca9|//depot/Main/largefile (glob)
  2|2|*|b0d5c1968efbabbff9d94160f284cd7b52686ca3c46cfffdd351de07384fce9c|//depot/Main/largefile (glob)

End Test

  stopping the p4 server
