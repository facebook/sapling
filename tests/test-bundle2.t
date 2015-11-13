  $ extpath=$(dirname $TESTDIR)
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > copytrace=$extpath/copytrace
  > rebase=
  > EOF


!! TEST 1: pulling move data from repo with 'hg pull' !!

SETUP SERVER REPO

  $ hg init serverrepo
  $ cd serverrepo
  $ touch a
  $ hg add a
  $ hg commit -m "add a"
  $ cd ..

SETUP CLIENT REPO

  $ hg clone serverrepo clientrepo
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

ADD MOVES IN SERVER

  $ cd serverrepo
  $ hg mv a b
  $ hg commit -m "mv a b"
  $ hg cp b c
  $ hg commit -m "cp b c"
  $ sqlite3 .hg/moves.db "SELECT * FROM Moves" | sort
  274c7e2c58b0256e17dc0f128380c8600bb0ee43|a|b|1
  274c7e2c58b0256e17dc0f128380c8600bb0ee43|||0
  4fe6b0cbea2cebfe016c553c782dcf8bedad63d5|b|c|0
  4fe6b0cbea2cebfe016c553c782dcf8bedad63d5|||1
  ac82d8b1f7c418c61a493ed229ffaa981bda8e90|||0
  ac82d8b1f7c418c61a493ed229ffaa981bda8e90|||1
  $ cd ..

PULLS FROM SERVER

  $ cd clientrepo
  $ hg pull
  pulling from $TESTTMP/serverrepo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  moves for 2 changesets retrieved
  (run 'hg update' to get a working copy)
  $ sqlite3 .hg/moves.db "SELECT * FROM Moves" | sort
  274c7e2c58b0256e17dc0f128380c8600bb0ee43|a|b|1
  274c7e2c58b0256e17dc0f128380c8600bb0ee43|||0
  4fe6b0cbea2cebfe016c553c782dcf8bedad63d5|b|c|0
  4fe6b0cbea2cebfe016c553c782dcf8bedad63d5|||1
  $ cd ..

SEVERAL BRANCHES ON SERVER

  $ cd serverrepo
  $ hg mv b d
  $ hg commit -m "mv b d"
  $ hg update .^ -q
  $ hg mv c e
  $ hg commit -m "mv c e"
  created new head
  $ hg log -G -T 'changeset: {node}\n desc: {desc}\n'
  @  changeset: b85e8d9fbcaad4fbdfee2a1fcf518629f66c8c66
  |   desc: mv c e
  | o  changeset: ec660297011163dd7658d444365b6590c0dd67b3
  |/    desc: mv b d
  o  changeset: 4fe6b0cbea2cebfe016c553c782dcf8bedad63d5
  |   desc: cp b c
  o  changeset: 274c7e2c58b0256e17dc0f128380c8600bb0ee43
  |   desc: mv a b
  o  changeset: ac82d8b1f7c418c61a493ed229ffaa981bda8e90
      desc: add a
  $ cd ..

PULLS FROM SERVER

  $ cd clientrepo
  $ hg pull
  pulling from $TESTTMP/serverrepo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files (+1 heads)
  moves for 2 changesets retrieved
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ sqlite3 .hg/moves.db "SELECT * FROM Moves" | sort
  274c7e2c58b0256e17dc0f128380c8600bb0ee43|a|b|1
  274c7e2c58b0256e17dc0f128380c8600bb0ee43|||0
  4fe6b0cbea2cebfe016c553c782dcf8bedad63d5|b|c|0
  4fe6b0cbea2cebfe016c553c782dcf8bedad63d5|||1
  b85e8d9fbcaad4fbdfee2a1fcf518629f66c8c66|c|e|1
  b85e8d9fbcaad4fbdfee2a1fcf518629f66c8c66|||0
  ec660297011163dd7658d444365b6590c0dd67b3|b|d|1
  ec660297011163dd7658d444365b6590c0dd67b3|||0
  $ cd ..
