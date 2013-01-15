  $ echo "[extensions]" >> $HGRCPATH
  $ echo "mq=" >> $HGRCPATH

  $ hg init a
  $ cd a
  $ echo a > a
  $ hg ci -Ama
  adding a

  $ hg qnew a.patch
  $ echo a >> a
  $ hg qrefresh

  $ hg qnew b.patch
  $ echo b > b
  $ hg add b
  $ hg qrefresh

  $ hg qnew c.patch
  $ echo c > c
  $ hg add c
  $ hg qrefresh

  $ hg qgoto a.patch
  popping c.patch
  popping b.patch
  now at: a.patch

  $ hg qgoto c.patch
  applying b.patch
  applying c.patch
  now at: c.patch

  $ hg qgoto b.patch
  popping c.patch
  now at: b.patch

Using index:

  $ hg qgoto 0
  popping b.patch
  now at: a.patch

  $ hg qgoto 2
  applying b.patch
  applying c.patch
  now at: c.patch

No warnings when using index ... and update from non-qtip and with pending
changes in unrelated files:

  $ hg qnew bug314159
  $ echo d >> c
  $ hg qrefresh
  $ hg qnew bug141421
  $ echo e >> b
  $ hg qrefresh

  $ hg up -r bug314159
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo f >> a
  $ echo f >> b
  $ echo f >> c

  $ hg qgoto 1
  abort: local changes found, refresh first
  [255]
  $ hg qgoto 1 -f
  popping bug141421
  popping bug314159
  popping c.patch
  now at: b.patch
  $ hg st
  M a
  M b
  ? c.orig
  $ hg up -qCr.

  $ hg qgoto 3
  applying c.patch
  applying bug314159
  now at: bug314159

Detect ambiguous non-index:

  $ hg qgoto 14
  patch name "14" is ambiguous:
    bug314159
    bug141421
  abort: patch 14 not in series
  [255]

  $ cd ..
