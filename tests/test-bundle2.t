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
  $ rm -rf serverrepo
  $ rm -rf clientrepo



!! TEST 2: pulling missing move data from repo when rebasing !!

SETUP SERVER REPO

  $ hg init serverrepo
  $ cd serverrepo
  $ touch a b
  $ hg add a b
  $ hg commit -m "add a b"
  $ hg mv a c
  $ hg commit -m "mv a c"
  $ hg mv c d
  $ hg commit -m "mv c d"
  $ hg mv d e
  $ hg commit -m "mv d e"
  $ cd ..

SETUP CLIENT REPO

  $ hg clone serverrepo clientrepo
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd clientrepo
  $ hg log -G -T 'changeset: {node}\n desc: {desc}\n'
  @  changeset: 2a998f0bae7cad015586b9a9e5e8a05b4b7d281f
  |   desc: mv d e
  o  changeset: d4670020b03d62be270c7f8c22d1bf620c4c8f4a
  |   desc: mv c d
  o  changeset: a003d50a0eea20c381b92e9200e323f3c945c473
  |   desc: mv a c
  o  changeset: 2f1222a290f07a1758cc927c57cc22805d6696ed
      desc: add a b
  $ hg update -q 2f1222
  $ hg mv b z
  $ hg commit -q -m "mv b z"
  $ hg log -G -T 'changeset: {node}\n desc: {desc}\n'
  @  changeset: d9e9933769659048c7efa24b53b2e38a1d8205b2
  |   desc: mv b z
  | o  changeset: 2a998f0bae7cad015586b9a9e5e8a05b4b7d281f
  | |   desc: mv d e
  | o  changeset: d4670020b03d62be270c7f8c22d1bf620c4c8f4a
  | |   desc: mv c d
  | o  changeset: a003d50a0eea20c381b92e9200e323f3c945c473
  |/    desc: mv a c
  o  changeset: 2f1222a290f07a1758cc927c57cc22805d6696ed
      desc: add a b
  $ sqlite3 .hg/moves.db "SELECT * FROM Moves" | sort
  d9e9933769659048c7efa24b53b2e38a1d8205b2|b|z|1
  d9e9933769659048c7efa24b53b2e38a1d8205b2|||0
  $ hg rebase -s d9e993 -d d46700
  pulling move data from $TESTTMP/serverrepo
  moves for 2 changesets retrieved
  rebasing 4:d9e993376965 "mv b z" (tip)
  saved backup bundle to $TESTTMP/clientrepo/.hg/strip-backup/d9e993376965-0332a78c-backup.hg (glob)
  $ hg log -G -T 'changeset: {node}\n desc: {desc}\n'
  @  changeset: daf6369e3e011c90ecd56144609c0e8fd823e83b
  |   desc: mv b z
  | o  changeset: 2a998f0bae7cad015586b9a9e5e8a05b4b7d281f
  |/    desc: mv d e
  o  changeset: d4670020b03d62be270c7f8c22d1bf620c4c8f4a
  |   desc: mv c d
  o  changeset: a003d50a0eea20c381b92e9200e323f3c945c473
  |   desc: mv a c
  o  changeset: 2f1222a290f07a1758cc927c57cc22805d6696ed
      desc: add a b
  $ sqlite3 .hg/moves.db "SELECT * FROM Moves" | sort
  0|a|d|0
  0|b|z|0
  0|||1
  a003d50a0eea20c381b92e9200e323f3c945c473|a|c|1
  a003d50a0eea20c381b92e9200e323f3c945c473|||0
  d4670020b03d62be270c7f8c22d1bf620c4c8f4a|c|d|1
  d4670020b03d62be270c7f8c22d1bf620c4c8f4a|||0
  d9e9933769659048c7efa24b53b2e38a1d8205b2|b|z|1
  d9e9933769659048c7efa24b53b2e38a1d8205b2|||0
  daf6369e3e011c90ecd56144609c0e8fd823e83b|b|z|1
  daf6369e3e011c90ecd56144609c0e8fd823e83b|||0

