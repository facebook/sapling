
  $ "$TESTDIR/hghave" inotify || exit 80
  $ hg init
  $ echo "[extensions]" >> $HGRCPATH
  $ echo "inotify=" >> $HGRCPATH

inserve

  $ hg inserve -d --pid-file=hg.pid
  $ cat hg.pid >> "$DAEMON_PIDS"

let the daemon finish its stuff

  $ sleep 1

empty

  $ hg debuginotify
  directories being watched:
    /
    .hg/
  $ mkdir a
  $ sleep 1

only 'a

  $ hg debuginotify
  directories being watched:
    /
    .hg/
    a/
  $ rmdir a
  $ sleep 1

empty again

  $ hg debuginotify
  directories being watched:
    /
    .hg/
  $ "$TESTDIR/killdaemons.py" hg.pid
