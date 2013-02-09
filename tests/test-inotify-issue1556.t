
  $ "$TESTDIR/hghave" inotify || exit 80
  $ hg init
  $ touch a b
  $ hg add a b
  $ rm b

status without inotify

  $ hg st
  A a
  ! b
  $ echo "[extensions]" >> $HGRCPATH
  $ echo "inotify=" >> $HGRCPATH

inserve

  $ hg inserve -d --pid-file=hg.pid 2>&1
  $ cat hg.pid >> "$DAEMON_PIDS"

status

  $ hg st
  A a
  ! b
  ? hg.pid
  $ sleep 1

Are we able to kill the service? if not, the service died on some error

  $ "$TESTDIR/killdaemons.py" hg.pid
