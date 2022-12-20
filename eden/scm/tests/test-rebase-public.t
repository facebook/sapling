#debugruntest-compatible
#chg-compatible

  $ configure modern
  $ enable rebase

Simple case:
  $ newrepo simple
  $ drawdag << 'EOS'
  >     Y
  >     |
  > X   B
  >  \ /
  >   A
  > EOS
  $ hg debugmakepublic $A::$B
  $ hg rebase -b $Y -d $X
  rebasing d2ed191ef6cc "Y"
  $ tglog
  o  089ea1cc331c 'Y'
  │
  o  bacc19fa7254 'X'
  │
  │ o  112478962961 'B'
  ├─╯
  o  426bada5c675 'A'
  

Rebasing public commits:
  $ newrepo publiccommits
  $ drawdag << 'EOS'
  > C
  > |
  > B X
  > |/
  > A
  > EOS
  $ hg debugmakepublic $A::$C
  $ hg rebase -b $C -d $X
  nothing to rebase from 26805aba1e60 to bacc19fa7254
  $ hg rebase -b $C -d $X --keep
  rebasing 112478962961 "B" (public/112478962961147124edd43549aedd1a335e44bf)
  rebasing 26805aba1e60 "C" (public/26805aba1e600a82e93661149f2313866a221a7b)
  $ tglog
  o  8c0795ffca60 'C'
  │
  o  a011a2c6f892 'B'
  │
  │ o  26805aba1e60 'C'
  │ │
  o │  bacc19fa7254 'X'
  │ │
  │ o  112478962961 'B'
  ├─╯
  o  426bada5c675 'A'
  

Rebasing commits with multiple ancestors:
  $ newrepo multiancestor
  $ drawdag << 'EOS'
  >       Z
  >      /|
  >     H I
  >     | |
  >     F G
  >    / /|
  > Y E / |
  >  \|/  D
  >   C  /
  > X | /
  >  \|/
  >   B
  >   |
  >   A
  > EOS
  $ hg debugmakepublic $A::$E
  $ cp -R ~/multiancestor ~/multiancestor2
  $ hg rebase -b $Z -d $Y
  rebasing bf8908ebeb46 "G"
  rebasing 93c4dd93872a "I"
  rebasing c4691414b4ba "F"
  rebasing ea3d77bc7811 "H"
  rebasing 234d68d8b772 "Z"
  $ tglog
  o    216e9d5e8aa1 'Z'
  ├─╮
  │ o  ef8bbe514f0a 'H'
  │ │
  │ o  28815e7e224c 'F'
  │ │
  o │  d5e21ad377c8 'I'
  │ │
  o │  1df87807a87f 'G'
  ├─╮
  │ o  cc52b456846a 'Y'
  │ │
  │ │ o  78d2dca436b2 'E'
  │ ├─╯
  │ │ o  c67c45f99acd 'X'
  │ │ │
  o │ │  be0ef73c17ad 'D'
  ├───╯
  │ o  26805aba1e60 'C'
  ├─╯
  o  112478962961 'B'
  │
  o  426bada5c675 'A'
  
  $ cd ~/multiancestor2
  $ hg rebase -b $Z -d $X
  rebasing be0ef73c17ad "D"
  rebasing bf8908ebeb46 "G"
  rebasing 93c4dd93872a "I"
  rebasing c4691414b4ba "F"
  rebasing ea3d77bc7811 "H"
  rebasing 234d68d8b772 "Z"
  rebasing cc52b456846a "Y"
  $ tglog
  o  57dd883fc0c4 'Y'
  │
  │ o    c44f10e2af60 'Z'
  │ ├─╮
  │ │ o  0ffa03c35c3d 'H'
  │ │ │
  │ │ o  f9b0592aa93c 'F'
  ├───╯
  │ o  6717d24e1395 'I'
  │ │
  │ o    1f90baba8207 'G'
  │ ├─╮
  │ │ o  e6571a8a635e 'D'
  ├───╯
  │ │ o  78d2dca436b2 'E'
  │ ├─╯
  o │  c67c45f99acd 'X'
  │ │
  │ o  26805aba1e60 'C'
  ├─╯
  o  112478962961 'B'
  │
  o  426bada5c675 'A'
