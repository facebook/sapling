#chg-compatible

  $ setconfig workingcopy.ruststatus=False
  $ hg init t
  $ cd t
  $ echo This is file a1 > a
  $ hg add a
  $ hg commit -m "commit #0"
  $ echo This is file b1 > b
  $ hg add b
  $ hg commit -m "commit #1"
  $ rm b
  $ hg goto 538afb845929a25888be4211c3e2195445e26b7e
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo This is file b2 > b
  $ hg add b
  $ hg commit -m "commit #2"
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
  $ hg goto 538afb845929a25888be4211c3e2195445e26b7e
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo This is file b2 > b
  $ hg commit -A -m "commit #2"
  adding b
  $ cd ..; rm -r t

  $ hg init t
  $ cd t
  $ echo This is file a1 > a
  $ hg add a
  $ hg commit -m "commit #0"
  $ echo This is file b1 > b
  $ hg add b
  $ hg commit -m "commit #1"
  $ rm b
  $ hg remove b
  $ hg goto 538afb845929a25888be4211c3e2195445e26b7e
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo This is file b2 > b
  $ hg commit -A -m "commit #2"
  adding b

  $ cd ..
