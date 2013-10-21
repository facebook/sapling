Check whether size of generaldelta revlog is not bigger than its
regular equivalent. Test would fail if generaldelta was naive
implementation of parentdelta: third manifest revision would be fully
inserted due to big distance from its paren revision (zero).

  $ hg init repo
  $ cd repo
  $ echo foo > foo
  $ echo bar > bar
  $ hg commit -q -Am boo
  $ hg clone --pull . ../gdrepo -q --config format.generaldelta=yes
  $ for r in 1 2 3; do
  >   echo $r > foo
  >   hg commit -q -m $r
  >   hg up -q -r 0
  >   hg pull . -q -r $r -R ../gdrepo
  > done

  $ cd ..
  >>> import os
  >>> regsize = os.stat("repo/.hg/store/00manifest.i").st_size
  >>> gdsize = os.stat("gdrepo/.hg/store/00manifest.i").st_size
  >>> if regsize < gdsize:
  ...     print 'generaldata increased size of manifest'
