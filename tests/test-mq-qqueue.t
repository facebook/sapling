  $ echo "[extensions]" >> $HGRCPATH
  $ echo "mq=" >> $HGRCPATH

  $ hg init foo
  $ cd foo
  $ echo a > a
  $ hg ci -qAm a

Default queue:

  $ hg qqueue
  patches (active)

  $ echo b > a
  $ hg qnew -fgDU somestuff

Applied patches in default queue:

  $ hg qap
  somestuff

Try to change patch (create succeeds, switch fails):

  $ hg qqueue foo --create
  abort: new queue created, but cannot make active as patches are applied
  [255]

  $ hg qqueue
  foo
  patches (active)

Empty default queue:

  $ hg qpop
  popping somestuff
  patch queue now empty

Switch queue:

  $ hg qqueue foo
  $ hg qqueue
  foo (active)
  patches

List queues, quiet:

  $ hg qqueue --quiet
  foo
  patches

Fail creating queue with already existing name:

  $ hg qqueue --create foo
  abort: queue "foo" already exists
  [255]

  $ hg qqueue
  foo (active)
  patches

Create new queue for rename:

  $ hg qqueue --create bar

  $ hg qqueue
  bar (active)
  foo
  patches

Rename queue, same name:

  $ hg qqueue --rename bar
  abort: can't rename "bar" to its current name
  [255]

Rename queue to existing:

  $ hg qqueue --rename foo
  abort: queue "foo" already exists
  [255]

Rename queue:

  $ hg qqueue --rename buz

  $ hg qqueue
  buz (active)
  foo
  patches

Switch back to previous queue:

  $ hg qqueue foo
  $ hg qqueue --delete buz

  $ hg qqueue
  foo (active)
  patches

Create queue for purge:

  $ hg qqueue --create purge-me

  $ hg qqueue
  foo
  patches
  purge-me (active)

Create patch for purge:

  $ hg qnew patch-purge-me

  $ ls -1d .hg/patches-purge-me 2>/dev/null || true
  .hg/patches-purge-me

  $ hg qpop -a
  popping patch-purge-me
  patch queue now empty

Purge queue:

  $ hg qqueue foo
  $ hg qqueue --purge purge-me

  $ hg qqueue
  foo (active)
  patches

  $ ls -1d .hg/patches-purge-me 2>/dev/null || true

Unapplied patches:

  $ hg qun
  $ echo c > a
  $ hg qnew -fgDU otherstuff

Fail switching back:

  $ hg qqueue patches
  abort: new queue created, but cannot make active as patches are applied
  [255]

Fail deleting current:

  $ hg qqueue foo --delete
  abort: cannot delete currently active queue
  [255]

Switch back and delete foo:

  $ hg qpop -a
  popping otherstuff
  patch queue now empty

  $ hg qqueue patches
  $ hg qqueue foo --delete
  $ hg qqueue
  patches (active)

Tricky cases:

  $ hg qqueue store --create
  $ hg qnew journal

  $ hg qqueue
  patches
  store (active)

  $ hg qpop -a
  popping journal
  patch queue now empty

  $ hg qqueue patches
  $ hg qun
  somestuff

Invalid names:

  $ hg qqueue test/../../bar --create
  abort: invalid queue name, may not contain the characters ":\/."
  [255]

  $ hg qqueue . --create
  abort: invalid queue name, may not contain the characters ":\/."
  [255]

  $ cd ..

