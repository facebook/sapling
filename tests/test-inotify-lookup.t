
  $ "$TESTDIR/hghave" inotify || exit 80
  $ hg init
  $ echo "[extensions]" > .hg/hgrc
  $ echo "inotify=" >> .hg/hgrc
  $ hg inserve -d --pid-file .hg/inotify.pid
  $ echo a > a
  $ hg ci -Aqm0
  $ hg co -q null
  $ hg co -q
  $ hg st
  $ cat a
  a
  $ kill `cat .hg/inotify.pid`
