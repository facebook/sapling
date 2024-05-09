#debugruntest-compatible

  $ configure modernclient mutation
  $ enable rebase amend undo
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

  $ hg rebase -qs $B -d $D

  $ tglog
  o  3827d805d9ee 'C'
  │
  o  680dc0c1d0a1 'B'
  │
  o  b18e25de2cf5 'D'
  │
  o  426bada5c675 'A'

  $ setconfig devel.default-date='1 0'

Undo and then redo the same rebase.
  $ hg undo -q
  $ hg rebase -qs $B -d $D

FIXME B and C have converged back into the same commits as above:
  $ tglog
  o  2d7470337ad2 'C'
  │
  o  0a2fc1549f72 'B'
  │
  o  b18e25de2cf5 'D'
  │
  o  426bada5c675 'A'
