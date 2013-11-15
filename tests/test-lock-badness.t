#if unix-permissions no-root
  $ hg init a
  $ echo a > a/a
  $ hg -R a ci -A -m a
  adding a

  $ hg clone a b
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ echo b > b/b
  $ hg -R b ci -A -m b
  adding b

  $ chmod 100 a/.hg/store

  $ hg -R b push a
  pushing to a
  abort: could not lock repository a: Permission denied
  [255]

  $ chmod 700 a/.hg/store
#endif
