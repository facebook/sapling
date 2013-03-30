  $ "$TESTDIR/hghave" killdaemons || exit 80

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

Cloning with a password in the URL should not save the password in .hg/hgrc:

  $ hg serve -p $HGPORT -d --pid-file=hg.pid -E errors.log
  $ cat hg.pid >> $DAEMON_PIDS
  $ hg clone http://foo:xyzzy@localhost:$HGPORT/ test3
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 1 files
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat test3/.hg/hgrc
  [paths]
  default = http://foo@localhost:$HGPORT/
  $ "$TESTDIR/killdaemons.py" $DAEMON_PIDS

expect error, cloning not allowed

  $ echo '[web]' > .hg/hgrc
  $ echo 'allowpull = false' >> .hg/hgrc
  $ hg serve -p $HGPORT -d --pid-file=hg.pid -E errors.log
  $ cat hg.pid >> $DAEMON_PIDS
  $ hg clone http://localhost:$HGPORT/ test4
  requesting all changes
  abort: authorization failed
  [255]
  $ "$TESTDIR/killdaemons.py" $DAEMON_PIDS

serve errors

  $ cat errors.log
  $ req() {
  >     hg serve -p $HGPORT -d --pid-file=hg.pid -E errors.log
  >     cat hg.pid >> $DAEMON_PIDS
  >     hg --cwd ../test pull http://localhost:$HGPORT/
  >     "$TESTDIR/killdaemons.py" hg.pid
  >     echo % serve errors
  >     cat errors.log
  > }

expect error, pulling not allowed

  $ req
  pulling from http://localhost:$HGPORT/
  abort: authorization failed
  % serve errors

  $ cd ..
