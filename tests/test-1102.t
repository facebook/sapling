  $ rm -rf a
  $ hg init a
  $ cd a
  $ echo a > a
  $ hg ci -Am0
  adding a
  $ hg tag t1 # 1
  $ hg tag --remove t1 # 2

  $ hg co 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg tag -f -r0 t1
  $ hg tags
  tip                                3:a49829c4fc11
  t1                                 0:f7b1eb17ad24

  $ cd ..
