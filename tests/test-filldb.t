  $ extpath=$(dirname $TESTDIR)
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > fbamend=$extpath/fbamend.py
  > copytrace=$extpath/copytrace
  > rebase=
  > EOF

Setup repo

  $ hg init repo
  $ cd repo
  $ touch a
  $ hg add a
  $ hg commit -m "add a"

Commit wrapping
  $ hg mv a b
  $ hg commit -m "mv a b"
  $ hg log -G -T 'changeset: {node}\n desc: {desc}\n'
  @  changeset: 274c7e2c58b0256e17dc0f128380c8600bb0ee43
  |   desc: mv a b
  o  changeset: ac82d8b1f7c418c61a493ed229ffaa981bda8e90
      desc: add a
  $ sqlite3 .hg/moves.db "SELECT * FROM Moves"
  274c7e2c58b0256e17dc0f128380c8600bb0ee43|a|b|1

  $ hg cp b c
  $ hg commit -m "cp b c"
  $ hg log -G -T 'changeset: {node}\n desc: {desc}\n'
  @  changeset: 4fe6b0cbea2cebfe016c553c782dcf8bedad63d5
  |   desc: cp b c
  o  changeset: 274c7e2c58b0256e17dc0f128380c8600bb0ee43
  |   desc: mv a b
  o  changeset: ac82d8b1f7c418c61a493ed229ffaa981bda8e90
      desc: add a
  $ sqlite3 .hg/moves.db "SELECT * FROM Moves"
  274c7e2c58b0256e17dc0f128380c8600bb0ee43|a|b|1
  4fe6b0cbea2cebfe016c553c782dcf8bedad63d5|b|c|0

Amend wrapping
  $ hg mv c d
  $ hg amend -q
  $ hg log -G -T 'changeset: {node}\n desc: {desc}\n'
  @  changeset: 146592ae004db0d4b3b2a89cee464aad083c8903
  |   desc: cp b c
  o  changeset: 274c7e2c58b0256e17dc0f128380c8600bb0ee43
  |   desc: mv a b
  o  changeset: ac82d8b1f7c418c61a493ed229ffaa981bda8e90
      desc: add a
  $ sqlite3 .hg/moves.db "SELECT * FROM Moves"
  274c7e2c58b0256e17dc0f128380c8600bb0ee43|a|b|1
  4fe6b0cbea2cebfe016c553c782dcf8bedad63d5|b|c|0
  8ba37d0eeb8342b7b32d318941aa0b005cd082b4|c|d|1
  146592ae004db0d4b3b2a89cee464aad083c8903|b|d|0

  $ cd ..
  $ rm -rf repo
  $ hg init repo
  $ cd repo
  $ touch a
  $ touch b
  $ hg add a b
  $ hg commit -m "add a b"

Rebase wrapping
  $ hg mv a c
  $ hg commit -m "mv a c"
  $ hg update -q .^
  $ hg mv b d
  $ hg commit -q -m "mv b d"
  $ hg log -G -T 'changeset: {node}\n desc: {desc}\n'
  @  changeset: 01cdd63d5282e9d0c3267de46b9f95f06786f454
  |   desc: mv b d
  | o  changeset: a003d50a0eea20c381b92e9200e323f3c945c473
  |/    desc: mv a c
  o  changeset: 2f1222a290f07a1758cc927c57cc22805d6696ed
      desc: add a b
  $ sqlite3 .hg/moves.db "SELECT * FROM Moves"
  a003d50a0eea20c381b92e9200e323f3c945c473|a|c|1
  01cdd63d5282e9d0c3267de46b9f95f06786f454|b|d|1

  $ hg rebase -q -s 01cdd6 -d a003d5
  $ hg log -G -T 'changeset: {node}\n desc: {desc}\n'
  @  changeset: 111a6d6f8ddc7309891f6e7ede7ba993125c4b54
  |   desc: mv b d
  o  changeset: a003d50a0eea20c381b92e9200e323f3c945c473
  |   desc: mv a c
  o  changeset: 2f1222a290f07a1758cc927c57cc22805d6696ed
      desc: add a b
  $ sqlite3 .hg/moves.db "SELECT * FROM Moves" | sort
  01cdd63d5282e9d0c3267de46b9f95f06786f454|b|d|1
  0|a|c|0
  0|b|d|0
  111a6d6f8ddc7309891f6e7ede7ba993125c4b54|b|d|1
  a003d50a0eea20c381b92e9200e323f3c945c473|a|c|1
