  $ hg init t
  $ cd t
  $ echo a > a
  $ hg add a
  $ hg commit -m test
  $ rm .hg/requires
  $ hg tip
  abort: index 00changelog.i unknown format 2!
  [255]
  $ echo indoor-pool > .hg/requires
  $ hg tip
  abort: unknown repository format: requires feature 'indoor-pool' (upgrade Mercurial)!
  [255]
