#chg-compatible
  $ setconfig experimental.nativecheckout=true
  $ newserver server

  $ newremoterepo myrepo

  $ echo a > a
  $ hg add a
  $ hg commit -m 'A'
  $ echo a > b
  $ hg add b
  $ hg commit -m 'B'
  $ hg up 'desc(A)'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo x > b
  $ hg up 'desc(B)'
  b: untracked file differs
  abort: untracked files in working directory differ from files in requested revision
  [255]
  $ hg up 'desc(B)' --clean
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
