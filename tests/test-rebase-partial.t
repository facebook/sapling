Tests rebasing with part of the rebase set already in the
destination (issue5422)

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > rebase=
  > drawdag=$TESTDIR/drawdag.py
  > 
  > [experimental]
  > evolution.createmarkers=True
  > evolution.allowunstable=True
  > 
  > [alias]
  > tglog = log -G --template "{rev}: {node|short} {desc}"
  > EOF

  $ rebasewithdag() {
  >   N=`$PYTHON -c "print($N+1)"`
  >   hg init repo$N && cd repo$N
  >   hg debugdrawdag
  >   hg rebase "$@" > _rebasetmp
  >   r=$?
  >   grep -v 'saved backup bundle' _rebasetmp
  >   [ $r -eq 0 ] && hg tglog
  >   cd ..
  >   return $r
  > }

Rebase two commits, of which one is already in the right place

  $ rebasewithdag -r C+D -d B <<EOF
  > C
  > |
  > B D
  > |/
  > A
  > EOF
  rebasing 2:b18e25de2cf5 "D" (D)
  already rebased 3:26805aba1e60 "C" (C tip)
  o  4: fe3b4c6498fa D
  |
  | o  3: 26805aba1e60 C
  |/
  | x  2: b18e25de2cf5 D
  | |
  o |  1: 112478962961 B
  |/
  o  0: 426bada5c675 A
  
Can collapse commits even if one is already in the right place

  $ rebasewithdag --collapse -r C+D -d B <<EOF
  > C
  > |
  > B D
  > |/
  > A
  > EOF
  rebasing 2:b18e25de2cf5 "D" (D)
  rebasing 3:26805aba1e60 "C" (C tip)
  o  4: a2493f4ace65 Collapsed revision
  |  * D
  |  * C
  | x  3: 26805aba1e60 C
  |/
  | x  2: b18e25de2cf5 D
  | |
  o |  1: 112478962961 B
  |/
  o  0: 426bada5c675 A
  
Rebase with "holes". The commits after the hole should end up on the parent of
the hole (B below), not on top of the destination (A).

  $ rebasewithdag -r B+D -d A <<EOF
  > D
  > |
  > C
  > |
  > B
  > |
  > A
  > EOF
  already rebased 1:112478962961 "B" (B)
  rebasing 3:f585351a92f8 "D" (D tip)
  o  4: 1e6da8103bc7 D
  |
  | x  3: f585351a92f8 D
  | |
  | o  2: 26805aba1e60 C
  |/
  o  1: 112478962961 B
  |
  o  0: 426bada5c675 A
  
