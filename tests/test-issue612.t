http://mercurial.selenic.com/bts/issue612

  $ hg init
  $ mkdir src
  $ echo a > src/a.c
  $ hg ci -Ama
  adding src/a.c

  $ hg mv src source
  moving src/a.c to source/a.c

  $ hg ci -Ammove

  $ hg co -C 0
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved

  $ echo new > src/a.c
  $ echo compiled > src/a.o
  $ hg ci -mupdate
  created new head

  $ hg status
  ? src/a.o

  $ hg merge
  merging src/a.c and source/a.c to source/a.c
  1 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ hg status
  M source/a.c
  R src/a.c
  ? source/a.o

