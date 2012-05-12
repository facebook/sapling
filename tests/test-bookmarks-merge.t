# init

  $ hg init
  $ echo a > a
  $ hg add a
  $ hg commit -m'a'
  $ echo b > b
  $ hg add b
  $ hg commit -m'b'
  $ hg up -C 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo c > c
  $ hg add c
  $ hg commit -m'c'
  created new head

# test merging of diverged bookmarks
  $ hg bookmark -r 1 "c@diverge"
  $ hg bookmark -r 1 b
  $ hg bookmark c
  $ hg bookmarks
     b                         1:d2ae7f538514
   * c                         2:d36c0562f908
     c@diverge                 1:d2ae7f538514
  $ hg merge "c@diverge"
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg commit -m'merge'
  $ hg bookmarks
     b                         1:d2ae7f538514
   * c                         3:b8f96cf4688b
