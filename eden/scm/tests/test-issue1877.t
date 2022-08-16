#chg-compatible
#debugruntest-compatible

https://bz.mercurial-scm.org/1877

  $ hg init a
  $ cd a
  $ echo a > a
  $ hg add a
  $ hg ci -m 'a'
  $ echo b > a
  $ hg ci -m'b'
  $ hg up 'desc(a)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg book main
  $ hg book
   * main                      cb9a9f314b8b
  $ echo c > c
  $ hg add c
  $ hg ci -m'c'
  $ hg book
   * main                      d36c0562f908
  $ hg heads
  commit:      d36c0562f908
  bookmark:    main
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     c
  
  commit:      1e6c11564562
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     b
  
  $ hg up 1e6c11564562
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (leaving bookmark main)
  $ hg merge main
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg book
     main                      d36c0562f908
  $ hg ci -m'merge'
  $ hg book
     main                      d36c0562f908

  $ cd ..
