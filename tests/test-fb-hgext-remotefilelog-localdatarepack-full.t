  $ . "$TESTDIR/library.sh"

  $ newserver master
  $ clone master client1
  $ cd client1
  $ echo x > x
  $ echo y > y
  $ hg commit -qAm x

  $ findfilessorted .hg/store/data

  $ hg repack --incremental --config remotefilelog.localdatarepack=True

  $ findfilessorted .hg/store/data
