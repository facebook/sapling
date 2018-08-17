  $ shorttraceback
  $ newrepo
  $ drawdag << 'EOS'
  > B
  > |
  > A
  > EOS

  $ recover() {
  >   rm .hg/dirstate
  >   hg debugrebuilddirstate -r null
  >   hg up -q $B
  > }

  $ hg up -q $B

Dirstate rebuild should work with a broken dirstate

Broken by having an incomplete p2

  $ enable blackbox
  >>> open('.hg/dirstate', 'a').truncate(25)
  $ hg debugrebuilddirstate
  $ hg log -r . -T '{desc}\n'
  B

Broken by deleting the tree

  $ rm -rf .hg/treestate
  $ hg debugrebuilddirstate
  abort: entity not found
  [255]

  $ recover

Dirstate rebuild should work with sparse

  $ enable sparse
  $ hg sparse -I A
  $ rm .hg/dirstate
  $ hg debugrebuilddirstate -r $B
  abort: cannot add 'B' - it is outside the sparse checkout
  (include file with `hg sparse include <pattern>` or use `hg add -s <file>` to include file directory while adding)
  [255]
