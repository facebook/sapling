http://mercurial.selenic.com/bts/issue433

  $ hg init
  $ echo a > a
  $ hg commit -Ama
  adding a

  $ hg parents -r 0 doesnotexist
  abort: 'doesnotexist' not found in manifest!
  [255]
