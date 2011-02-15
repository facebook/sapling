
  $ hg init test
  $ cd test
  $ echo a > a
  $ hg ci -Ama
  adding a
  $ cd ..
  $ hg clone test test2
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd test2
  $ echo a >> a
  $ hg ci -mb

expect error, cloning not allowed

  $ echo '[web]' > .hg/hgrc
  $ echo 'allowpull = false' >> .hg/hgrc
  $ hg serve -p $HGPORT -d --pid-file=hg.pid -E errors.log
  $ cat hg.pid >> $DAEMON_PIDS
  $ hg clone http://localhost:$HGPORT/ test3
  requesting all changes
  abort: authorization failed
  [255]
  $ "$TESTDIR/killdaemons.py"

serve errors

  $ cat errors.log
  $ req() {
  >     hg serve -p $HGPORT -d --pid-file=hg.pid -E errors.log
  >     cat hg.pid >> $DAEMON_PIDS
  >     hg --cwd ../test pull http://localhost:$HGPORT/
  >     kill `cat hg.pid`
  >     echo % serve errors
  >     cat errors.log
  > }

expect error, pulling not allowed

  $ req
  pulling from http://localhost:$HGPORT/
  searching for changes
  abort: authorization failed
  % serve errors
