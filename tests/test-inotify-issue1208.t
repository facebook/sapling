
  $ "$TESTDIR/hghave" inotify || exit 80
  $ echo "[extensions]" >> $HGRCPATH
  $ echo "inotify=" >> $HGRCPATH
  $ p="xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"
  $ hg init $p
  $ cd $p

dangling symlink should be deleted

  $ ln -sf doesnotexist .hg/inotify.sock
  $ hg --config inotify.pidfile=../hg.pid st
  $ kill `cat ../hg.pid`

inserve

  $ hg inserve -d --pid-file=hg.pid
  $ cat hg.pid >> "$DAEMON_PIDS"

status

  $ hg status
  ? hg.pid

if we try to start twice the server, make sure we get a correct error

  $ hg inserve -d --pid-file=hg2.pid
  abort: inotify-server: cannot start: socket is already bound
  abort: child process failed to start
  [255]
  $ kill `cat hg.pid`

  $ cd ..
