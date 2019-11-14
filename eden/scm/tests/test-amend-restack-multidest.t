  $ . helpers-usechg.sh

  $ enable amend rebase
  $ setconfig experimental.evolution.allowdivergence=True
  $ setconfig experimental.evolution="createmarkers, allowunstable"
  $ mkcommit() {
  >   echo "$1" > "$1"
  >   hg add "$1"
  >   hg ci -m "add $1"
  > }

Restack does topological sort and only rebases "D" once:

  $ newrepo
  $ hg debugdrawdag<<'EOS'
  > D
  > |
  > C
  > |
  > B
  > |
  > A
  > EOS
  $ hg update B -q
  $ hg commit --amend -m B2 -q --no-rebase 2>/dev/null
  $ hg tag --local B2
  $ hg rebase -r C -d B2 -q
  $ hg commit --amend -m B3 -q --no-rebase 2>/dev/null
  $ hg tag --local B3
  $ showgraph
  @  6 da1d4fe88e84 B3
  |
  | o  5 ca53c8ceb284 C
  | |
  | x  4 fdcbd16a7d51 B2
  |/
  | o  3 f585351a92f8 D
  | |
  | x  2 26805aba1e60 C
  | |
  | x  1 112478962961 B
  |/
  o  0 426bada5c675 A
  $ hg rebase --restack
  rebasing ca53c8ceb284 "C"
  rebasing f585351a92f8 "D" (D)
  $ showgraph
  o  8 981f3734c126 D
  |
  o  7 bab9c1b0a249 C
  |
  @  6 da1d4fe88e84 B3
  |
  | x  4 fdcbd16a7d51 B2
  |/
  | x  3 f585351a92f8 D
  | |
  | x  2 26805aba1e60 C
  | |
  | x  1 112478962961 B
  |/
  o  0 426bada5c675 A

Restack will only restack the "current" stack and leave other stacks untouched.

  $ newrepo
  $ hg debugdrawdag<<'EOS'
  >  D   H   K
  >  |   |   |
  >  B C F G J L    # amend: B -> C
  >  |/  |/  |/     # amend: F -> G
  >  A   E   I   Z  # amend: J -> L
  > EOS

  $ hg phase --public -r Z+I+A+E

  $ hg update -q Z
  $ hg rebase --restack
  nothing to restack
  [1]

  $ hg update -q D
  $ hg rebase --restack
  rebasing be0ef73c17ad "D" (D)

  $ hg update -q G
  $ hg rebase --restack
  rebasing cc209258a732 "H" (H)

  $ hg update -q I
  $ hg rebase --restack
  rebasing 59760668f0e1 "K" (K)

  $ rm .hg/localtags
  $ showgraph
  o  15 c97827ce80f6 K
  |
  | o  14 47528c67632b H
  | |
  | | o  13 5cb8c357af9e D
  | | |
  o | |  9 a975bfef72d2 L
  | | |
  | o |  7 889f49cd29f6 G
  | | |
  | | o  5 dc0947a82db8 C
  | | |
  | | | o  3 48b9aae0607f Z
  | | |
  @ | |  2 02a9ac6a13a6 I
   / /
  o /  1 e8e0a81d950f E
   /
  o  0 426bada5c675 A

The "prune" cases.

  $ newrepo
  $ hg debugdrawdag<<'EOS'
  > D E
  > |/
  > C
  > |       # amend: F -> F2
  > B  G H  # prune: A, C, F2
  > |  |/
  > A  F F2
  > EOS

  $ hg update -q B
  $ hg rebase --restack
  rebasing 112478962961 "B" (B)
  rebasing f585351a92f8 "D" (D)
  rebasing 78d2dca436b2 "E" (E tip)

  $ hg update -q H
  $ hg rebase --restack
  rebasing 8fdb2c1feb20 "G" (G)
  rebasing 02ac06fe83b9 "H" (H)

  $ rm .hg/localtags
  $ showgraph
  @  13 3e1fefc3c8db H
  
  o  12 0706cfb95b41 G
  
  o  11 8c0ccd1582b3 E
  |
  | o  10 f88ac1d7b477 D
  |/
  o  9 653ee58caf75 B



Restack could resume after resolving merge conflicts.

  $ newrepo
  $ hg debugdrawdag<<'EOS'
  >  F   G    # F/C = F # cause conflict
  >  |   |    # G/E = G # cause conflict
  >  B C D E  # amend: B -> C
  >  |/  |/   # amend: D -> E
  >  |   /
  >  |  /
  >  | /
  >  |/
  >  A
  > EOS

  $ hg update -q F
  $ hg rebase --restack
  rebasing ed8545a5c22a "F" (F)
  merging C
  warning: 1 conflicts while merging C! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

  $ rm .hg/localtags

  $ echo R > C
  $ hg resolve --mark -q
  continue: hg rebase --continue
  $ hg rebase --continue
  rebasing ed8545a5c22a "F"
  rebasing 4d1ef7d890c5 "G" (tip)
  merging E
  warning: 1 conflicts while merging E! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

  $ echo R > E
  $ hg resolve --mark -q
  continue: hg rebase --continue
  $ hg rebase --continue
  already rebased ed8545a5c22a "F" as 2282fe522d5c
  rebasing 4d1ef7d890c5 "G"

  $ showgraph
  o  8 3b00517bf275 G
  |
  | @  7 2282fe522d5c F
  | |
  o |  4 7fb047a69f22 E
  | |
  | o  2 dc0947a82db8 C
  |/
  o  0 426bada5c675 A

