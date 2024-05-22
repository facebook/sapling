  $ configure modernclient mutation
  $ enable rebase amend undo
  $ setconfig rebase.reproducible-commits=true

#testcases in-memory not-in-memory

#if in-memory
  $ setconfig rebase.experimental.inmemory=true
#else
  $ setconfig rebase.experimental.inmemory=false
#endif

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
  o  29bb7e1252bc 'C'
  │
  o  9f482d67f5e1 'B'
  │
  o  b18e25de2cf5 'D'
  │
  o  426bada5c675 'A'

  $ setconfig devel.default-date='1 0'

Undo and then redo the same rebase.
  $ hg undo -q
  $ hg rebase -qs $B -d $D

B and C have converged back into the same commits as above:
  $ tglog
  o  29bb7e1252bc 'C'
  │
  o  9f482d67f5e1 'B'
  │
  o  b18e25de2cf5 'D'
  │
  o  426bada5c675 'A'
