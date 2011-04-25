  $ echo "[extensions]" >> $HGRCPATH
  $ echo "mq=" >> $HGRCPATH

  $ hg init a
  $ cd a

  $ echo 'base' > base
  $ hg ci -Ambase -d '1 0'
  adding base

  $ hg qnew -d '1 0' a
  $ hg qnew -d '1 0' b
  $ hg qnew -d '1 0' c

  $ hg qdel
  abort: qdelete requires at least one revision or patch name
  [255]

  $ hg qdel c
  abort: cannot delete applied patch c
  [255]

  $ hg qpop
  popping c
  now at: b

Delete the same patch twice in one command (issue2427)

  $ hg qdel c c

  $ hg qseries
  a
  b

  $ ls .hg/patches
  a
  b
  series
  status

  $ hg qpop
  popping b
  now at: a

  $ hg qdel -k 1

  $ ls .hg/patches
  a
  b
  series
  status

  $ hg qdel -r a
  patch a finalized without changeset message

  $ hg qapplied

  $ hg log --template '{rev} {desc}\n'
  1 [mq]: a
  0 base

  $ hg qnew d
  $ hg qnew e
  $ hg qnew f

  $ hg qdel -r e
  abort: cannot delete revision 3 above applied patches
  [255]

  $ hg qdel -r qbase:e
  patch d finalized without changeset message
  patch e finalized without changeset message

  $ hg qapplied
  f

  $ hg log --template '{rev} {desc}\n'
  4 [mq]: f
  3 [mq]: e
  2 [mq]: d
  1 [mq]: a
  0 base

  $ cd ..

  $ hg init b
  $ cd b

  $ echo 'base' > base
  $ hg ci -Ambase -d '1 0'
  adding base

  $ hg qfinish
  abort: no revisions specified
  [255]

  $ hg qfinish -a
  no patches applied

  $ hg qnew -d '1 0' a
  $ hg qnew -d '1 0' b
  $ hg qnew c # XXX fails to apply by /usr/bin/patch if we put a date

  $ hg qfinish 0
  abort: revision 0 is not managed
  [255]

  $ hg qfinish b
  abort: cannot delete revision 2 above applied patches
  [255]

  $ hg qpop
  popping c
  now at: b

  $ hg qfinish -a c
  abort: unknown revision 'c'!
  [255]

  $ hg qpush
  applying c
  patch c is empty
  now at: c

  $ hg qfinish qbase:b
  patch a finalized without changeset message
  patch b finalized without changeset message

  $ hg qapplied
  c

  $ hg log --template '{rev} {desc}\n'
  3 imported patch c
  2 [mq]: b
  1 [mq]: a
  0 base

  $ hg qfinish -a c
  patch c finalized without changeset message

  $ hg qapplied

  $ hg log --template '{rev} {desc}\n'
  3 imported patch c
  2 [mq]: b
  1 [mq]: a
  0 base

  $ ls .hg/patches
  series
  status

qdel -k X && hg qimp -e X used to trigger spurious output with versioned queues

  $ hg init --mq
  $ hg qimport -r 3
  $ hg qpop
  popping 3.diff
  patch queue now empty
  $ hg qdel -k 3.diff
  $ hg qimp -e 3.diff
  adding 3.diff to series file
  $ hg qfinish -a
  no patches applied


resilience to inconsistency: qfinish -a with applied patches not in series

  $ hg qser
  3.diff
  $ hg qapplied
  $ hg qpush
  applying 3.diff
  patch 3.diff is empty
  now at: 3.diff
  $ echo next >>  base
  $ hg qrefresh -d '1 0'
  $ echo > .hg/patches/series # remove 3.diff from series to confuse mq
  $ hg qfinish -a
  revision c4dd2b624061 refers to unknown patches: 3.diff

more complex state 'both known and unknown patches

  $ echo hip >>  base
  $ hg qnew -f -d '1 0' -m 4 4.diff
  $ echo hop >>  base
  $ hg qnew -f -d '1 0' -m 5 5.diff
  $ echo > .hg/patches/series # remove 4.diff and 5.diff from series to confuse mq
  $ echo hup >>  base
  $ hg qnew -f -d '1 0' -m 6 6.diff
  $ hg qfinish -a
  revision 6fdec4b20ec3 refers to unknown patches: 5.diff
  revision 2ba51db7ba24 refers to unknown patches: 4.diff

