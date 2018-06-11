#testcases treestate-on treestate-off

#if treestate-on
  $ setconfig format.usetreestate=1
#else
  $ setconfig format.usetreestate=0
#endif

Setup

  $ hg init repo
  $ cd repo
  $ echo base > base
  $ hg add base
  $ hg commit -m "base"

Deliberately corrupt the dirstate.

  $ dd if=/dev/zero bs=4096 count=1 of=.hg/dirstate 2> /dev/null
  $ hg debugrebuilddirstate
