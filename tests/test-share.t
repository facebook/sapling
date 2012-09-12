  $ "$TESTDIR/hghave" killdaemons || exit 80

  $ echo "[extensions]"      >> $HGRCPATH
  $ echo "share = "          >> $HGRCPATH

prepare repo1

  $ hg init repo1
  $ cd repo1
  $ echo a > a
  $ hg commit -A -m'init'
  adding a

share it

  $ cd ..
  $ hg share repo1 repo2
  updating working directory
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

share shouldn't have a store dir

  $ cd repo2
  $ test -d .hg/store
  [1]

Some sed versions appends newline, some don't, and some just fails

  $ cat .hg/sharedpath; echo
  $TESTTMP/repo1/.hg (glob)

trailing newline on .hg/sharedpath is ok
  $ hg tip -q
  0:d3873e73d99e
  $ echo '' >> .hg/sharedpath
  $ cat .hg/sharedpath
  $TESTTMP/repo1/.hg (glob)
  $ hg tip -q
  0:d3873e73d99e

commit in shared clone

  $ echo a >> a
  $ hg commit -m'change in shared clone'

check original

  $ cd ../repo1
  $ hg log
  changeset:   1:8af4dc49db9e
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     change in shared clone
  
  changeset:   0:d3873e73d99e
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     init
  
  $ hg update
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat a             # should be two lines of "a"
  a
  a

commit in original

  $ echo b > b
  $ hg commit -A -m'another file'
  adding b

check in shared clone

  $ cd ../repo2
  $ hg log
  changeset:   2:c2e0ac586386
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     another file
  
  changeset:   1:8af4dc49db9e
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     change in shared clone
  
  changeset:   0:d3873e73d99e
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     init
  
  $ hg update
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat b             # should exist with one "b"
  b

hg serve shared clone

  $ hg serve -n test -p $HGPORT -d --pid-file=hg.pid
  $ cat hg.pid >> $DAEMON_PIDS
  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT 'raw-file/'
  200 Script output follows
  
  
  -rw-r--r-- 4 a
  -rw-r--r-- 2 b
  
  

test unshare command

  $ hg unshare
  $ test -d .hg/store
  $ test -f .hg/sharedpath
  [1]
  $ hg unshare
  abort: this is not a shared repo
  [255]

check that a change does not propagate

  $ echo b >> b
  $ hg commit -m'change in unshared'
  $ cd ../repo1
  $ hg id -r tip
  c2e0ac586386 tip

  $ cd ..

Explicitly kill daemons to let the test exit on Windows

  $ "$TESTDIR/killdaemons.py" $DAEMON_PIDS

