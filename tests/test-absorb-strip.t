Do not strip innocent children. See https://bitbucket.org/facebook/hg-experimental/issues/6/hg-absorb-merges-diverged-commits

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > absorb=
  > EOF

  $ hg init
  $ hg debugdrawdag << EOF
  > E
  > |
  > D F
  > |/
  > C
  > |
  > B
  > |
  > A
  > EOF

  $ hg up E -q
  $ echo 1 >> B
  $ echo 2 >> D
  $ hg absorb -a
  showing changes for B
          @@ -0,1 +0,1 @@
  1124789 -B
  1124789 +B1
  showing changes for D
          @@ -0,1 +0,1 @@
  f585351 -D
  f585351 +D2
  
  2 changesets affected
  f585351 D
  1124789 B
  2 of 2 chunks applied

  $ hg log -G -T '{desc}'
  @  E
  |
  o  D
  |
  o  C
  |
  o  B
  |
  | x  E
  | |
  | | o  F
  | | |
  | x |  D
  | |/
  | x  C
  | |
  | x  B
  |/
  o  A
  
