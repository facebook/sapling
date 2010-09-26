
  $ "$TESTDIR/hghave" inotify || exit 80
  $ hg init
  $ touch a
  $ mkdir dir
  $ touch dir/b
  $ touch dir/c
  $ echo "[extensions]" >> $HGRCPATH
  $ echo "inotify=" >> $HGRCPATH
  $ hg add dir/c

inserve

  $ hg inserve -d --pid-file=hg.pid 2>&1
  $ cat hg.pid >> "$DAEMON_PIDS"
  $ hg st
  A dir/c
  ? a
  ? dir/b
  ? hg.pid

moving dir out

  $ mv dir ../tmp-test-inotify-issue1542

status

  $ hg st
  ! dir/c
  ? a
  ? hg.pid
  $ sleep 1

Are we able to kill the service? if not, the service died on some error

  $ kill `cat hg.pid`
