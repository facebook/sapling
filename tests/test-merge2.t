  $ mkdir t
  $ cd t
  $ hg init
  $ echo This is file a1 > a
  $ hg add a
  $ hg commit -m "commit #0"
  $ echo This is file b1 > b
  $ hg add b
  $ hg commit -m "commit #1"
  $ rm b
  $ hg update 0
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo This is file b2 > b
  $ hg add b
  $ hg commit -m "commit #2"
  created new head
  $ cd ..; rm -r t

  $ mkdir t
  $ cd t
  $ hg init
  $ echo This is file a1 > a
  $ hg add a
  $ hg commit -m "commit #0"
  $ echo This is file b1 > b
  $ hg add b
  $ hg commit -m "commit #1"
  $ rm b
  $ hg update 0
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo This is file b2 > b
  $ hg commit -A -m "commit #2"
  adding b
  created new head
  $ cd ..; rm -r t

  $ mkdir t
  $ cd t
  $ hg init
  $ echo This is file a1 > a
  $ hg add a
  $ hg commit -m "commit #0"
  $ echo This is file b1 > b
  $ hg add b
  $ hg commit -m "commit #1"
  $ rm b
  $ hg remove b
  $ hg update 0
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo This is file b2 > b
  $ hg commit -A -m "commit #2"
  adding b
  created new head
