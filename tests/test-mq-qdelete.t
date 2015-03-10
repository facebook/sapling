  $ echo "[extensions]" >> $HGRCPATH
  $ echo "mq=" >> $HGRCPATH

  $ hg init a
  $ cd a

  $ echo 'base' > base
  $ hg ci -Ambase -d '1 0'
  adding base

  $ hg qnew -d '1 0' pa
  $ hg qnew -d '1 0' pb
  $ hg qnew -d '1 0' pc

  $ hg qdel
  abort: qdelete requires at least one revision or patch name
  [255]

  $ hg qdel pc
  abort: cannot delete applied patch pc
  [255]

  $ hg qpop
  popping pc
  now at: pb

Delete the same patch twice in one command (issue2427)

  $ hg qdel pc pc

  $ hg qseries
  pa
  pb

  $ ls .hg/patches
  pa
  pb
  series
  status

  $ hg qpop
  popping pb
  now at: pa

  $ hg qdel -k 1

  $ ls .hg/patches
  pa
  pb
  series
  status

  $ hg qdel -r pa
  patch pa finalized without changeset message

  $ hg qapplied

  $ hg log --template '{rev} {desc}\n'
  1 [mq]: pa
  0 base

  $ hg qnew pd
  $ hg qnew pe
  $ hg qnew pf

  $ hg qdel -r pe
  abort: cannot delete revision 3 above applied patches
  [255]

  $ hg qdel -r qbase:pe
  patch pd finalized without changeset message
  patch pe finalized without changeset message

  $ hg qapplied
  pf

  $ hg log --template '{rev} {desc}\n'
  4 [mq]: pf
  3 [mq]: pe
  2 [mq]: pd
  1 [mq]: pa
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

  $ hg qnew -d '1 0' pa
  $ hg qnew -d '1 0' pb
  $ hg qnew pc # XXX fails to apply by /usr/bin/patch if we put a date

  $ hg qfinish 0
  abort: revision 0 is not managed
  [255]

  $ hg qfinish pb
  abort: cannot delete revision 2 above applied patches
  [255]

  $ hg qpop
  popping pc
  now at: pb

  $ hg qfinish -a pc
  abort: unknown revision 'pc'!
  [255]

  $ hg qpush
  applying pc
  patch pc is empty
  now at: pc

  $ hg qfinish qbase:pb
  patch pa finalized without changeset message
  patch pb finalized without changeset message

  $ hg qapplied
  pc

  $ hg log --template '{rev} {desc}\n'
  3 imported patch pc
  2 [mq]: pb
  1 [mq]: pa
  0 base

  $ hg qfinish -a pc
  patch pc finalized without changeset message

  $ hg qapplied

  $ hg log --template '{rev} {desc}\n'
  3 imported patch pc
  2 [mq]: pb
  1 [mq]: pa
  0 base

  $ ls .hg/patches
  series
  status

qdel -k X && hg qimp -e X used to trigger spurious output with versioned queues

  $ hg init --mq
  $ hg qimport -r 3
  $ hg qpop
  popping imported_patch_pc
  patch queue now empty
  $ hg qdel -k imported_patch_pc
  $ hg qimp -e imported_patch_pc
  adding imported_patch_pc to series file
  $ hg qfinish -a
  no patches applied


resilience to inconsistency: qfinish -a with applied patches not in series

  $ hg qser
  imported_patch_pc
  $ hg qapplied
  $ hg qpush
  applying imported_patch_pc
  patch imported_patch_pc is empty
  now at: imported_patch_pc
  $ echo next >>  base
  $ hg qrefresh -d '1 0'
  $ echo > .hg/patches/series # remove 3.diff from series to confuse mq
  $ hg qfinish -a
  revision 47dfa8501675 refers to unknown patches: imported_patch_pc

more complex state 'both known and unknown patches

  $ echo hip >>  base
  $ hg qnew -f -d '1 0' -m 4 4.diff
  $ echo hop >>  base
  $ hg qnew -f -d '1 0' -m 5 5.diff
  $ echo > .hg/patches/series # remove 4.diff and 5.diff from series to confuse mq
  $ echo hup >>  base
  $ hg qnew -f -d '1 0' -m 6 6.diff
  $ echo pup > base
  $ hg qfinish -a
  warning: uncommitted changes in the working directory
  revision 2b1c98802260 refers to unknown patches: 5.diff
  revision 33a6861311c0 refers to unknown patches: 4.diff

  $ cd ..
