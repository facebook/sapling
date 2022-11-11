#chg-compatible

  $ configure modernclient
  $ . "$TESTDIR/library.sh"

Setup the server

  $ newclientrepo master

Setup the client

  $ newclientrepo client test:master_server

Make some commits

  $ cd ../master
  $ mkdir subdir
  $ echo a >> subdir/foo
  $ hg commit -Aqm 'a > subdir/foo'
  $ echo b >> subdir/foo
  $ hg commit -Aqm 'b >> subdir/foo'
  $ echo c >> subdir/foo
  $ hg commit -Aqm 'c >> subdir/foo'
  $ echo d >> subdir/foo
  $ hg commit -Aqm 'd >> subdir/foo'
  $ hg push --to master --create -q

Test that log -p downloads each tree using the prior tree as a base

  $ cd ../client
  $ hg pull -q -B master
  $ hg up master
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob) (?)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg log -p 1>/dev/null
  3 files fetched over 3 fetches - (3 misses, 0.00% hit ratio) over * (glob) (?)
