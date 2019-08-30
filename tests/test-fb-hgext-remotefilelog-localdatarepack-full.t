  $ . "$TESTDIR/library.sh"

  $ newserver master
  $ clone master client1
  $ cd client1
  $ echo x > x
  $ echo y > y
  $ hg commit -qAm x

  $ findfilessorted .hg/store/data
  .hg/store/data/11f6ad8ec52a2984abaafd7c3b516503785c2072/1406e74118627694268417491f018a4a883152f0
  .hg/store/data/11f6ad8ec52a2984abaafd7c3b516503785c2072/filename
  .hg/store/data/95cb0bfd2977c761298d9624e4b4d4c72a39974a/076f5e2225b3ff0400b98c92aa6cdf403ee24cca
  .hg/store/data/95cb0bfd2977c761298d9624e4b4d4c72a39974a/filename

  $ hg repack --incremental --config remotefilelog.localdatarepack=True

  $ findfilessorted .hg/store/data
