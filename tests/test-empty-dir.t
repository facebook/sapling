  $ hg init

  $ echo 123 > a
  $ hg add a
  $ hg commit -m "first" a

  $ mkdir sub
  $ echo 321 > sub/b
  $ hg add sub/b
  $ hg commit -m "second" sub/b

  $ cat sub/b
  321

  $ hg co 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved

  $ cat sub/b 2>/dev/null || echo "sub/b not present"
  sub/b not present

  $ test -d sub || echo "sub not present"
  sub not present

