issues when status queries are issued when dirstate is dirty

  $ "$TESTDIR/hghave" inotify || exit 80
  $ echo "[extensions]" >> $HGRCPATH
  $ echo "inotify=" >> $HGRCPATH
  $ echo "fetch=" >> $HGRCPATH

issue1810: inotify and fetch

  $ hg init test; cd test
  $ hg inserve -d --pid-file=../hg.pid
  $ cat ../hg.pid >> "$DAEMON_PIDS"
  $ echo foo > foo
  $ hg add
  adding foo
  $ hg ci -m foo
  $ cd ..
  $ hg --config "inotify.pidfile=../hg2.pid" clone test test2
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat ../hg2.pid >> "$DAEMON_PIDS"
  $ cd test2
  $ echo bar > bar
  $ hg add
  adding bar
  $ hg ci -m bar
  $ cd ../test
  $ echo spam > spam
  $ hg add
  adding spam
  $ hg ci -m spam
  $ cd ../test2
  $ hg st

abort, outstanding changes

  $ hg fetch -q
  $ hg st
  $ cd ..

issue1719: inotify and mq

  $ echo "mq=" >> $HGRCPATH
  $ hg init test-1719
  $ cd test-1719

inserve

  $ hg inserve -d --pid-file=../hg-test-1719.pid
  $ cat ../hg-test-1719.pid >> "$DAEMON_PIDS"
  $ echo content > file
  $ hg add file
  $ hg qnew -f test.patch
  $ hg status
  $ hg qpop
  popping test.patch
  patch queue now empty

st should not output anything

  $ hg status
  $ hg qpush
  applying test.patch
  now at: test.patch

st should not output anything

  $ hg status
  $ hg qrefresh
  $ hg status

  $ cd ..
