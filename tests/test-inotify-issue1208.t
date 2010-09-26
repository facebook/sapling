
  $ "$TESTDIR/hghave" inotify || exit 80
  $ echo "[extensions]" >> $HGRCPATH
  $ echo "inotify=" >> $HGRCPATH
  $ p="xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"
  $ hg init $p
  $ cd $p

fail

  $ ln -sf doesnotexist .hg/inotify.sock
  $ hg st
  abort: inotify-server: cannot start: .hg/inotify.sock is a broken symlink
  inotify-client: could not start inotify server: child process failed to start
  $ hg inserve
  abort: inotify-server: cannot start: .hg/inotify.sock is a broken symlink
  [255]
  $ rm .hg/inotify.sock

inserve

  $ hg inserve -d --pid-file=hg.pid
  $ cat hg.pid >> "$DAEMON_PIDS"

status

  $ hg status
  ? hg.pid
  $ kill `cat hg.pid`
