
  $ "$TESTDIR/hghave" inotify || exit 80
  $ hg init
  $ touch a b c d e f
  $ echo "[extensions]" >> $HGRCPATH
  $ echo "inotify=" >> $HGRCPATH

inserve

  $ hg inserve -d --pid-file=hg.pid 2>&1
  $ cat hg.pid >> "$DAEMON_PIDS"
  $ hg ci -Am m
  adding a
  adding b
  adding c
  adding d
  adding e
  adding f
  adding hg.pid

let the daemon finish its stuff

  $ sleep 1

eed to test all file operations

  $ hg rm a
  $ rm b
  $ echo c >> c
  $ touch g
  $ hg add g
  $ hg mv e h
  $ hg status
  M c
  A g
  A h
  R a
  R e
  ! b
  $ sleep 1

Are we able to kill the service? if not, the service died on some error

  $ "$TESTDIR/killdaemons.py" hg.pid
