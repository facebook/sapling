  $ mkdir t
  $ cd t
  $ hg init
  $ echo a > a
  $ hg add a
  $ hg commit -m test -d "1000000 0"
  $ rm .hg/requires
  $ hg tip
  abort: index 00changelog.i unknown format 2!
  $ echo indoor-pool > .hg/requires
  $ hg tip
  abort: requirement 'indoor-pool' not supported!

  $ true
