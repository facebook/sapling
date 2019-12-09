#chg-compatible

  $ enable rebase obsstore
  $ setconfig experimental.evolution.allowdivergence=1

  $ newrepo
  $ drawdag <<'EOS'
  >    D    # D is orphaned.
  >    |
  > C2 C C1 # amend: C -> C1 -> C2
  >   \|/
  >    B Z
  >    |/
  >    A
  > EOS

  $ hg rebase -s $B -d $Z
  rebasing 112478962961 "B"
  rebasing 039c3379aaa9 "C2"
  rebasing 26805aba1e60 "C"
  rebasing f585351a92f8 "D" (tip)
  $ hg log -G -T '{node|short} {desc} {obsfate}'
  o  f7f4f5b9173a D
  |
  x  e709467ba6ed C rewritten using copy as 8:b97425e89b0c
  |
  | o  b97425e89b0c C2
  |/
  o  d74d19e598c8 B
  |
  o  262e37e34f63 Z
  |
  o  426bada5c675 A
  
