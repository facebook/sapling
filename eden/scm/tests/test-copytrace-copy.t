
  $ eagerepo
  $ enable rebase

should merge changes of a into the copy file b

  $ newrepo copies
  $ drawdag << 'EOS'
  > P   # P/b=a\n (copied from a)
  > |
  > | Y # Y/a=a\na\n
  > |/
  > |
  > |   # X/a=a\n
  > X
  >     # drawdag.defaultfiles=false
  > EOS

  $ hg rebase -r $P -d $Y
  rebasing bd0f2fa014aa "P"
  merging a and b to b
  $ hg cat b -r tip
  a
  a
