Test hangup signal in the middle of transaction

  $ "$TESTDIR/hghave" serve fifo || exit 80
  $ hg init
  $ mkfifo p
  $ hg serve --stdio < p &
  $ P=$!

Do test while holding fifo open

  $ (
  > echo lock
  > echo addchangegroup
  > while [ ! -e .hg/store/00changelog.i.a ]; do true; done
  > kill -HUP $P
  > while kill -0 $P 2>/dev/null; do true; done
  > ) > p
  0
  0
  adding changesets
  transaction abort!
  rollback completed
  killed!

  $ echo .hg/* .hg/store/*
  .hg/00changelog.i .hg/journal.bookmarks .hg/journal.branch .hg/journal.desc .hg/journal.dirstate .hg/requires .hg/store .hg/store/00changelog.i .hg/store/00changelog.i.a .hg/store/journal.phaseroots
