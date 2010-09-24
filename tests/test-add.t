  $ hg init a
  $ cd a
  $ echo a > a
  $ hg add -n
  adding a
  $ hg st
  ? a
  $ hg add
  adding a
  $ hg st
  A a
  $ hg forget a
  $ hg add
  adding a
  $ hg st
  A a

  $ echo b > b
  $ hg add -n b
  $ hg st
  A a
  ? b
  $ hg add b
  $ hg st
  A a
  A b

should fail

  $ hg add b
  b already tracked!
  $ hg st
  A a
  A b

  $ hg ci -m 0 --traceback

should fail

  $ hg add a
  a already tracked!

  $ echo aa > a
  $ hg ci -m 1
  $ hg up 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo aaa > a
  $ hg ci -m 2
  created new head

  $ hg merge
  merging a
  warning: conflicts during merge.
  merging a failed!
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ hg st
  M a
  ? a.orig

should fail

  $ hg add a
  a already tracked!
  $ hg st
  M a
  ? a.orig
  $ hg resolve -m a
  $ hg ci -m merge

Issue683: peculiarity with hg revert of an removed then added file

  $ hg forget a
  $ hg add a
  $ hg st
  ? a.orig
  $ hg rm a
  $ hg st
  R a
  ? a.orig
  $ echo a > a
  $ hg add a
  $ hg st
  M a
  ? a.orig

  $ hg add c && echo "unexpected addition of missing file"
  c: No such file or directory
  [1]
  $ echo c > c
  $ hg add d c && echo "unexpected addition of missing file"
  d: No such file or directory
  [1]
  $ hg st
  M a
  A c
  ? a.orig

