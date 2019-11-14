  $ shorttraceback
  $ newrepo
  $ drawdag << 'EOS'
  > B
  > |
  > A
  > EOS

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
  warning: failed to inspect working copy parent
  warning: failed to inspect working copy parent
  $ hg log -r . -T '{desc}\n'
  B

Dirstate rebuild should work with sparse

  $ enable sparse
  $ hg sparse -I A
  $ rm .hg/dirstate
  $ hg debugrebuilddirstate -r $B
  $ hg log -r . -T '{desc}\n'
  B
