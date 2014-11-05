#require serve fifo

Test hangup signal in the middle of transaction

  $ hg init
  $ mkfifo p
  $ hg serve --stdio < p 1>out 2>&1 &
  $ P=$!

Do test while holding fifo open

  $ (
  > echo lock
  > echo addchangegroup
  > start=`date +%s`
  > # 10 second seems much enough to let the server catch up
  > deadline=`expr $start + 10`
  > while [ ! -s .hg/store/journal ]; do
  >     sleep 0;
  >     if [ `date +%s` -gt $deadline ]; then
  >         echo "transaction did not start after 10 seconds" >&2;
  >         exit 1;
  >     fi
  > done
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

  $ ls -1d .hg/* .hg/store/*
  .hg/00changelog.i
  .hg/journal.bookmarks
  .hg/journal.branch
  .hg/journal.desc
  .hg/journal.dirstate
  .hg/requires
  .hg/store
  .hg/store/00changelog.i
  .hg/store/00changelog.i.a
  .hg/store/journal.phaseroots
