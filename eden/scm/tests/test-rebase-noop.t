#debugruntest-compatible

  $ configure modernclient
  $ enable rebase amend
  $ setconfig tweakdefaults.rebasekeepdate=true

  $ newclientrepo
  $ drawdag <<EOS
  >   C
  >   |
  > D B
  > |/
  > A
  > EOS

  $ tglog
  o  26805aba1e60 'C'
  │
  │ o  b18e25de2cf5 'D'
  │ │
  o │  112478962961 'B'
  ├─╯
  o  426bada5c675 'A'

  $ hg rebase -qs $B -d $D -k

  $ tglog
  o  ffeec75ec603 'C'
  │
  o  1ef11233b74d 'B'
  │
  │ o  26805aba1e60 'C'
  │ │
  o │  b18e25de2cf5 'D'
  │ │
  │ o  112478962961 'B'
  ├─╯
  o  426bada5c675 'A'

  $ setconfig devel.default-date='1 0'

  $ hg rebase -qs $B -d $D

B and C have converged back into the same commits:
  $ tglog
  o  ffeec75ec603 'C'
  │
  o  1ef11233b74d 'B'
  │
  o  b18e25de2cf5 'D'
  │
  o  426bada5c675 'A'
