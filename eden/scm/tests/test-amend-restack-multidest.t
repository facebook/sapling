#chg-compatible
#debugruntest-compatible


  $ configure mutation-norecord
  $ enable amend rebase
  $ mkcommit() {
  >   echo "$1" > "$1"
  >   hg add "$1"
  >   hg ci -m "add $1"
  > }

Restack does topological sort and only rebases "D" once:

  $ newrepo
  $ drawdag<<'EOS'
  > D
  > |
  > C
  > |
  > B
  > |
  > A
  > EOS
  $ hg goto $B -q
  $ hg commit --amend -m B2 -q --no-rebase 2>/dev/null
  $ B2=`hg log -r . -T '{node}'`
  $ hg rebase -r $C -d $B2 -q
  $ hg commit --amend -m B3 -q --no-rebase 2>/dev/null
  $ B3=`hg log -r . -T '{node}'`
  $ showgraph
  @  da1d4fe88e84 B3
  │
  │ o  ca53c8ceb284 C
  │ │
  │ x  fdcbd16a7d51 B2
  ├─╯
  │ o  f585351a92f8 D
  │ │
  │ x  26805aba1e60 C
  │ │
  │ x  112478962961 B
  ├─╯
  o  426bada5c675 A
  $ hg rebase --restack
  rebasing ca53c8ceb284 "C"
  rebasing f585351a92f8 "D"
  $ showgraph
  o  981f3734c126 D
  │
  o  bab9c1b0a249 C
  │
  @  da1d4fe88e84 B3
  │
  o  426bada5c675 A

Restack will only restack the "current" stack and leave other stacks untouched.

  $ newrepo
  $ drawdag<<'EOS'
  >  D   H   K
  >  |   |   |
  >  B C F G J L    # amend: B -> C
  >  |/  |/  |/     # amend: F -> G
  >  A   E   I   Z  # amend: J -> L
  > EOS

  $ hg debugmakepublic -r $Z+$I+$A+$E

  $ hg goto -q $Z
  $ hg rebase --restack
  nothing to restack
  [1]

  $ hg goto -q $D
  $ hg rebase --restack
  rebasing be0ef73c17ad "D"

  $ hg goto -q $G
  $ hg rebase --restack
  rebasing cc209258a732 "H"

  $ hg goto -q $I
  $ hg rebase --restack
  rebasing 59760668f0e1 "K"

  $ showgraph
  o  c97827ce80f6 K
  │
  │ o  47528c67632b H
  │ │
  │ │ o  5cb8c357af9e D
  │ │ │
  o │ │  a975bfef72d2 L
  │ │ │
  │ o │  889f49cd29f6 G
  │ │ │
  │ │ o  dc0947a82db8 C
  │ │ │
  │ │ │ o  48b9aae0607f Z
  │ │ │
  @ │ │  02a9ac6a13a6 I
    │ │
    o │  e8e0a81d950f E
      │
      o  426bada5c675 A


Restack could resume after resolving merge conflicts.

  $ newrepo
  $ drawdag<<'EOS'
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

  $ hg goto -q $F
  $ hg rebase --restack
  rebasing ed8545a5c22a "F"
  merging C
  warning: 1 conflicts while merging C! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

  $ echo R > C
  $ hg resolve --mark -q
  continue: hg rebase --continue
  $ hg rebase --continue
  rebasing ed8545a5c22a "F"
  rebasing 4d1ef7d890c5 "G"
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
  o  3b00517bf275 G
  │
  │ @  2282fe522d5c F
  │ │
  o │  7fb047a69f22 E
  │ │
  │ o  dc0947a82db8 C
  ├─╯
  o  426bada5c675 A

