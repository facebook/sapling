
  $ eagerepo
  $ setconfig commands.update.check=none

  $ sl init t
  $ cd t
  $ echo This is file a1 > a
  $ sl add a
  $ sl commit -m "commit #0"
  $ echo This is file b1 > b
  $ sl add b
  $ sl commit -m "commit #1"
  $ rm b
  $ sl goto 538afb845929a25888be4211c3e2195445e26b7e
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo This is file b2 > b
  $ sl add b
  $ sl commit -m "commit #2"
  $ cd ..; rm -r t

  $ mkdir t
  $ cd t
  $ sl init
  $ echo This is file a1 > a
  $ sl add a
  $ sl commit -m "commit #0"
  $ echo This is file b1 > b
  $ sl add b
  $ sl commit -m "commit #1"
  $ rm b
  $ sl goto 538afb845929a25888be4211c3e2195445e26b7e
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo This is file b2 > b
  $ sl commit -A -m "commit #2"
  adding b
  $ cd ..; rm -r t

  $ sl init t
  $ cd t
  $ echo This is file a1 > a
  $ sl add a
  $ sl commit -m "commit #0"
  $ echo This is file b1 > b
  $ sl add b
  $ sl commit -m "commit #1"
  $ rm b
  $ sl remove b
  $ sl goto 538afb845929a25888be4211c3e2195445e26b7e
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo This is file b2 > b
  $ sl commit -A -m "commit #2"
  adding b

  $ cd ..
