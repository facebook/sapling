  $ . "$TESTDIR/copytrace.sh"
  $ extpath=$(dirname $TESTDIR)
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > fbamend=$extpath/fbamend.py
  > copytrace=$extpath/copytrace
  > rebase=
  > EOF

Setup repo

  $ hg init repo
  $ initclient repo
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
  $ sqlite3 .hg/moves.db "SELECT hash, source, destination, mv FROM Moves" | sort
  274c7e2c58b0256e17dc0f128380c8600bb0ee43|a|b|1
  274c7e2c58b0256e17dc0f128380c8600bb0ee43|||0
  ac82d8b1f7c418c61a493ed229ffaa981bda8e90|||0
  ac82d8b1f7c418c61a493ed229ffaa981bda8e90|||1

  $ hg cp b c
  $ hg commit -m "cp b c"
  $ hg log -G -T 'changeset: {node}\n desc: {desc}\n'
  @  changeset: 4fe6b0cbea2cebfe016c553c782dcf8bedad63d5
  |   desc: cp b c
  o  changeset: 274c7e2c58b0256e17dc0f128380c8600bb0ee43
  |   desc: mv a b
  o  changeset: ac82d8b1f7c418c61a493ed229ffaa981bda8e90
      desc: add a
  $ sqlite3 .hg/moves.db "SELECT hash, source, destination, mv FROM Moves" | sort
  274c7e2c58b0256e17dc0f128380c8600bb0ee43|a|b|1
  274c7e2c58b0256e17dc0f128380c8600bb0ee43|||0
  4fe6b0cbea2cebfe016c553c782dcf8bedad63d5|b|c|0
  4fe6b0cbea2cebfe016c553c782dcf8bedad63d5|||1
  ac82d8b1f7c418c61a493ed229ffaa981bda8e90|||0
  ac82d8b1f7c418c61a493ed229ffaa981bda8e90|||1

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
  $ sqlite3 .hg/moves.db "SELECT hash, source, destination, mv FROM Moves" | sort
  146592ae004db0d4b3b2a89cee464aad083c8903|b|d|0
  146592ae004db0d4b3b2a89cee464aad083c8903|||1
  274c7e2c58b0256e17dc0f128380c8600bb0ee43|a|b|1
  274c7e2c58b0256e17dc0f128380c8600bb0ee43|||0
  4fe6b0cbea2cebfe016c553c782dcf8bedad63d5|b|c|0
  4fe6b0cbea2cebfe016c553c782dcf8bedad63d5|||1
  8ba37d0eeb8342b7b32d318941aa0b005cd082b4|c|d|1
  8ba37d0eeb8342b7b32d318941aa0b005cd082b4|||0
  ac82d8b1f7c418c61a493ed229ffaa981bda8e90|||0
  ac82d8b1f7c418c61a493ed229ffaa981bda8e90|||1

  $ cd ..
  $ rm -rf repo

  $ hg init repo
  $ initclient repo
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
  $ sqlite3 .hg/moves.db "SELECT hash, source, destination, mv FROM Moves" | sort
  01cdd63d5282e9d0c3267de46b9f95f06786f454|b|d|1
  01cdd63d5282e9d0c3267de46b9f95f06786f454|||0
  2f1222a290f07a1758cc927c57cc22805d6696ed|||0
  2f1222a290f07a1758cc927c57cc22805d6696ed|||1
  a003d50a0eea20c381b92e9200e323f3c945c473|a|c|1
  a003d50a0eea20c381b92e9200e323f3c945c473|||0

  $ hg rebase -q -s 01cdd6 -d a003d5
  $ hg log -G -T 'changeset: {node}\n desc: {desc}\n'
  @  changeset: 111a6d6f8ddc7309891f6e7ede7ba993125c4b54
  |   desc: mv b d
  o  changeset: a003d50a0eea20c381b92e9200e323f3c945c473
  |   desc: mv a c
  o  changeset: 2f1222a290f07a1758cc927c57cc22805d6696ed
      desc: add a b
  $ sqlite3 .hg/moves.db "SELECT hash, source, destination, mv FROM Moves" | sort
  01cdd63d5282e9d0c3267de46b9f95f06786f454|b|d|1
  01cdd63d5282e9d0c3267de46b9f95f06786f454|||0
  111a6d6f8ddc7309891f6e7ede7ba993125c4b54|b|d|1
  111a6d6f8ddc7309891f6e7ede7ba993125c4b54|||0
  2f1222a290f07a1758cc927c57cc22805d6696ed|||0
  2f1222a290f07a1758cc927c57cc22805d6696ed|||1
  a003d50a0eea20c381b92e9200e323f3c945c473|a|c|1
  a003d50a0eea20c381b92e9200e323f3c945c473|||0

Manually adding missing move data
  $ hg update -q .^
  $ hg mv c e
  $ hg commit -m "mv c e" -q
  $ rm .hg/moves.db
  $ hg rebase -d 111a6d
  rebasing 3:2aac4892fdfa "mv c e" (tip)
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/2aac4892fdfa-55937866-backup.hg (glob)
  $ sqlite3 .hg/moves.db "SELECT hash, source, destination, mv FROM Moves" | sort
  111a6d6f8ddc7309891f6e7ede7ba993125c4b54|b|d|1
  111a6d6f8ddc7309891f6e7ede7ba993125c4b54|||0
  2aac4892fdfa122108364670f6cd740a1e0bbd05|c|e|1
  2aac4892fdfa122108364670f6cd740a1e0bbd05|||0
  502dd38c92fc5edffa608131206446b1fbee879b|c|e|1
  502dd38c92fc5edffa608131206446b1fbee879b|||0

