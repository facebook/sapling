Test hangup signal in the middle of transaction

  $ "$TESTDIR/hghave" serve fifo || exit 80
  $ hg init
  $ mkfifo p
  $ hg serve --stdio < p 1>out 2>&1 &
  $ P=$!

Do test while holding fifo open

  $ (
  > echo lock
  > echo addchangegroup
  > while [ ! -s .hg/store/journal ]; do sleep 0; done
  > kill -HUP $P
  > ) > p

  $ wait
  $ cat out
  0
  0
  adding changesets
  transaction abort!
  rollback completed
  killed!

  $ echo .hg/* .hg/store/*
  .hg/00changelog.i .hg/journal.bookmarks .hg/journal.branch .hg/journal.desc .hg/journal.dirstate .hg/requires .hg/store .hg/store/00changelog.i .hg/store/00changelog.i.a .hg/store/journal.phaseroots
