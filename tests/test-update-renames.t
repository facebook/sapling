Test update logic when there are renames

Update with local changes across a file rename

  $ hg init

  $ echo a > a
  $ hg add a
  $ hg ci -m a

  $ hg mv a b
  $ hg ci -m rename

  $ echo b > b
  $ hg ci -m change

  $ hg up -q 0

  $ echo c > a

  $ hg up
  merging a and b to b
  warning: conflicts during merge.
  merging b incomplete! (edit conflicts, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges
  [1]
