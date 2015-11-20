  $ . "$TESTDIR/copytrace.sh"
  $ extpath=$(dirname $TESTDIR)
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > copytrace=$extpath/copytrace
  > rebase=
  > EOF


SETUP SERVER REPO

  $ hg init serverrepo
  $ initserver serverrepo

SETUP CLIENT REPOS

  $ hg clone serverrepo clientrepo1
  updating to branch default
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ initclient clientrepo1
  $ hg clone serverrepo clientrepo2
  updating to branch default
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ initclient clientrepo2

PUSH MOVES FROM CLIENT1

  $ cd clientrepo1
  $ touch a
  $ hg add -q a
  $ hg commit -q -m "add a"
  $ hg mv a b
  $ hg commit -m "mv a b"
  $ hg mv b c
  $ hg commit -m "mv b c"
  $ sqlite3 .hg/moves.db "SELECT hash, source, destination, mv FROM Moves" | sort
  274c7e2c58b0256e17dc0f128380c8600bb0ee43|a|b|1
  274c7e2c58b0256e17dc0f128380c8600bb0ee43|||0
  89c7ee4b298e2371d470910ff5a4ecce28ee49d9|b|c|1
  89c7ee4b298e2371d470910ff5a4ecce28ee49d9|||0
  ac82d8b1f7c418c61a493ed229ffaa981bda8e90|||0
  ac82d8b1f7c418c61a493ed229ffaa981bda8e90|||1
  $ hg push
  pushing to $TESTTMP/serverrepo
  searching for changes
  moves for 3 changesets pushed
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 3 files
  $ cd ..

PULLS IN CLIENT2

  $ cd clientrepo2
  $ hg pull
  pulling from $TESTTMP/serverrepo
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 3 files
  moves for 3 changesets retrieved
  (run 'hg update' to get a working copy)
  $ sqlite3 .hg/moves.db "SELECT hash, source, destination, mv FROM Moves" | sort
  274c7e2c58b0256e17dc0f128380c8600bb0ee43|a|b|1
  274c7e2c58b0256e17dc0f128380c8600bb0ee43|||0
  89c7ee4b298e2371d470910ff5a4ecce28ee49d9|b|c|1
  89c7ee4b298e2371d470910ff5a4ecce28ee49d9|||0
  ac82d8b1f7c418c61a493ed229ffaa981bda8e90|||0
  ac82d8b1f7c418c61a493ed229ffaa981bda8e90|||1
  $ cd ..

REQUESTS MISSING MOVES DURING REBASE

  $ cd clientrepo1
  $ rm .hg/moves.db
  $ sqlite3 .hg/moves.db "SELECT hash, source, destination, mv FROM Moves" | sort
  Error: no such table: Moves
  $ hg up -q ac82d8
  $ hg mv a c
  $ hg commit -m "mv a c" -q
  $ sqlite3 .hg/moves.db "SELECT hash, source, destination, mv FROM Moves" | sort
  9c11d01510faa13840e36ea2d8acdd0b126cca67|a|c|1
  9c11d01510faa13840e36ea2d8acdd0b126cca67|||0
  $ hg rebase -s 9c11d0 -d 274c7e
  pulling move data from $TESTTMP/serverrepo
  moves for 1 changesets retrieved
  rebasing 3:9c11d01510fa "mv a c" (tip)
  note: possible conflict - a was renamed multiple times to:
   b
   c
  saved backup bundle to $TESTTMP/clientrepo1/.hg/strip-backup/9c11d01510fa-7a2b0d59-backup.hg (glob)
  $ sqlite3 .hg/moves.db "SELECT hash, source, destination, mv FROM Moves" | sort
  0|||0
  0|||1
  11a19c2eb2258207a4ebaf0c7223ad340046b4c7|||0
  11a19c2eb2258207a4ebaf0c7223ad340046b4c7|||1
  274c7e2c58b0256e17dc0f128380c8600bb0ee43|a|b|1
  274c7e2c58b0256e17dc0f128380c8600bb0ee43|||0
  9c11d01510faa13840e36ea2d8acdd0b126cca67|a|c|1
  9c11d01510faa13840e36ea2d8acdd0b126cca67|||0
  $ cd ..
