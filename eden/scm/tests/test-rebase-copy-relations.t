#chg-compatible

  $ enable rebase
  $ setconfig experimental.evolution.allowdivergence=1

  $ newrepo repo
  $ drawdag <<'EOS'
  >    D    # D is orphaned.
  >    |
  > C2 C C1 # amend: C -> C1 -> C2
  >   \|/
  >    B Z
  >    |/
  >    A
  > EOS

  $ cp -R ../repo ../repob

C -> C2 relation is copied with singletransaction.

  $ hg rebase -s $B -d $Z --config rebase.singletransaction=true
  rebasing 112478962961 "B"
  rebasing 039c3379aaa9 "C2"
  rebasing 26805aba1e60 "C"
  rebasing f585351a92f8 "D"
  $ hg log -G -T '{node|short} {desc} {mutations%"{operation} to {successors}"}'
  o  f7f4f5b9173a D
  │
  x  e709467ba6ed C amend-copy to b97425e89b0c
  │
  │ o  b97425e89b0c C2
  ├─╯
  o  d74d19e598c8 B
  │
  o  262e37e34f63 Z
  │
  o  426bada5c675 A
  
FIXME: This does not quite work yet without singletransaction.

  $ cd $TESTTMP/repob
  $ hg rebase -s $B -d $Z --config rebase.singletransaction=false
  rebasing 112478962961 "B"
  rebasing 039c3379aaa9 "C2"
  rebasing 26805aba1e60 "C"
  rebasing f585351a92f8 "D"
  $ hg log -G -T '{node|short} {desc}'
  o  f7f4f5b9173a D
  │
  o  e709467ba6ed C
  │
  │ o  b97425e89b0c C2
  ├─╯
  o  d74d19e598c8 B
  │
  o  262e37e34f63 Z
  │
  o  426bada5c675 A
  
