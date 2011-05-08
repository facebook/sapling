Test hangup signal in the middle of transaction

  $ "$TESTDIR/hghave" fifo || exit 80
  $ hg init
  $ mkfifo p
  $ hg serve --stdio < p &
  $ P=$!
  $ (echo lock; echo addchangegroup; sleep 5) > p &
  $ Q=$!
  $ sleep 3
  0
  0
  adding changesets
  $ kill -HUP $P
  $ wait
  transaction abort!
  rollback completed
  killed!
  $ echo .hg/* .hg/store/*
  .hg/00changelog.i .hg/journal.bookmarks .hg/journal.branch .hg/journal.desc .hg/journal.dirstate .hg/requires .hg/store .hg/store/00changelog.i .hg/store/00changelog.i.a
