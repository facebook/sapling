  $ . "$TESTDIR/library.sh"

  $ newserver master
  $ clone master client1
  $ cd client1
  $ echo x > x
  $ echo y > y
  $ hg commit -qAm x

  $ [ -d .hg/store/data ]
  [1]

  $ hg repack --incremental --config remotefilelog.localdatarepack=True

  $ [ -d .hg/store/data ]
  [1]
