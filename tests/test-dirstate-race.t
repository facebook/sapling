  $ hg init repo
  $ cd repo
  $ echo a > a
  $ hg add a
  $ hg commit -m test

Do we ever miss a sub-second change?:

  $ for i in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20; do
  >     hg co -qC 0
  >     echo b > a
  >     hg st
  > done
  M a
  M a
  M a
  M a
  M a
  M a
  M a
  M a
  M a
  M a
  M a
  M a
  M a
  M a
  M a
  M a
  M a
  M a
  M a
  M a

  $ echo test > b
  $ mkdir dir1
  $ echo test > dir1/c
  $ echo test > d

  $ echo test > e
#if execbit
A directory will typically have the execute bit -- make sure it doesn't get
confused with a file with the exec bit set
  $ chmod +x e
#endif

  $ hg add b dir1 d e
  adding dir1/c (glob)
  $ hg commit -m test2

  $ cat >> $TESTTMP/dirstaterace.py << EOF
  > from mercurial import (
  >     context,
  >     extensions,
  > )
  > def extsetup():
  >     extensions.wrapfunction(context.workingctx, '_checklookup', overridechecklookup)
  > def overridechecklookup(orig, self, files):
  >     # make an update that changes the dirstate from underneath
  >     self._repo.ui.system(self._repo.ui.config('dirstaterace', 'command'), cwd=self._repo.root)
  >     return orig(self, files)
  > EOF

  $ hg debugrebuilddirstate
  $ hg debugdirstate
  n   0         -1 unset               a
  n   0         -1 unset               b
  n   0         -1 unset               d
  n   0         -1 unset               dir1/c
  n   0         -1 unset               e

XXX Note that this returns M for files that got replaced by directories. This is
definitely a bug, but the fix for that is hard and the next status run is fine
anyway.

  $ hg status --config extensions.dirstaterace=$TESTTMP/dirstaterace.py \
  >   --config dirstaterace.command='rm b && rm -r dir1 && rm d && mkdir d && rm e && mkdir e'
  M d
  M e
  ! b
  ! dir1/c
  $ hg debugdirstate
  n 644          2 * a (glob)
  n   0         -1 unset               b
  n   0         -1 unset               d
  n   0         -1 unset               dir1/c
  n   0         -1 unset               e

  $ hg status
  ! b
  ! d
  ! dir1/c
  ! e
