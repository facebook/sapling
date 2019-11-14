
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > fastannotate=
  > EOF

  $ hg init repo
  $ cd repo
  $ for i in 0 1 2 3 4; do
  >   echo $i >> a
  >   echo $i >> b
  >   hg commit -A -m $i a b
  > done

use the "debugbuildannotatecache" command to build annotate cache at rev 0

  $ hg debugbuildannotatecache --debug --config fastannotate.mainbranch=0
  fastannotate: a: 1 new changesets in the main branch
  fastannotate: b: 1 new changesets in the main branch

"debugbuildannotatecache" should work with broken cache (and other files would
be built without being affected). note: linelog being broken is only noticed
when we try to append to it.

  $ echo 'CORRUPT!' >> .hg/fastannotate/default/a.m
  $ hg debugbuildannotatecache --debug --config fastannotate.mainbranch=1
  fastannotate: a: rebuilding broken cache
  fastannotate: a: 2 new changesets in the main branch
  fastannotate: b: 1 new changesets in the main branch

  $ echo 'CANNOT REUSE!' > .hg/fastannotate/default/a.l
  $ hg debugbuildannotatecache --debug --config fastannotate.mainbranch=2
  fastannotate: a: rebuilding broken cache
  fastannotate: a: 3 new changesets in the main branch
  fastannotate: b: 1 new changesets in the main branch

  $ rm .hg/fastannotate/default/a.m
  $ hg debugbuildannotatecache --debug --config fastannotate.mainbranch=3
  fastannotate: a: rebuilding broken cache
  fastannotate: a: 4 new changesets in the main branch
  fastannotate: b: 1 new changesets in the main branch

  $ rm .hg/fastannotate/default/a.l
  $ hg debugbuildannotatecache --debug --config fastannotate.mainbranch=3
  $ hg debugbuildannotatecache --debug --config fastannotate.mainbranch=4
  fastannotate: a: rebuilding broken cache
  fastannotate: a: 5 new changesets in the main branch
  fastannotate: b: 1 new changesets in the main branch

"fastannotate" should deal with file corruption as well

  $ rm -rf .hg/fastannotate
  $ hg fastannotate --debug -r 0 a
  fastannotate: a: 1 new changesets in the main branch
  0: 0

  $ echo 'CORRUPT!' >> .hg/fastannotate/default/a.m
  $ hg fastannotate --debug -r 0 a
  fastannotate: a: cache broken and deleted
  fastannotate: a: 1 new changesets in the main branch
  0: 0

  $ echo 'CORRUPT!' > .hg/fastannotate/default/a.l
  $ hg fastannotate --debug -r 1 a
  fastannotate: a: cache broken and deleted
  fastannotate: a: 2 new changesets in the main branch
  0: 0
  1: 1

  $ rm .hg/fastannotate/default/a.l
  $ hg fastannotate --debug -r 1 a
  fastannotate: a: using fast path (resolved fctx: True)
  fastannotate: a: cache broken and deleted
  fastannotate: a: 2 new changesets in the main branch
  0: 0
  1: 1

  $ rm .hg/fastannotate/default/a.m
  $ hg fastannotate --debug -r 2 a
  fastannotate: a: cache broken and deleted
  fastannotate: a: 3 new changesets in the main branch
  0: 0
  1: 1
  2: 2
