  $ mkdir t
  $ cd t
  $ hg init
  $ echo a > a
  $ hg add a
  $ hg commit -m test
  $ rm .hg/requires
  $ hg tip
  abort: index 00changelog.i unknown format 2!
  [255]
  $ echo indoor-pool > .hg/requires
  $ hg tip
  abort: requirement 'indoor-pool' not supported!
  [255]
