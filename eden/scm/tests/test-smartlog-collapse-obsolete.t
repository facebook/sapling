#chg-compatible

  $ enable rebase smartlog

  $ newrepo
  $ drawdag << 'EOS'
  > G
  > |
  > F
  > |
  > E
  > |
  > D
  > |
  > C
  > |
  > B Z
  > |/
  > A
  > EOS
  $ hg bookmark -r $Z master
  $ hg bookmark -r $G old
  $ hg bookmark -r $F new
  $ hg rebase -qr $C::$F -d $Z

The obsoleted C::F should be collapsed:

  $ hg sl -T '{desc}' --config smartlog.collapse-obsolete=true
  o  F
  |
  o  E
  |
  o  D
  |
  o  C
  |
  o  Z
  |
  | o  G
  | |
  | x  F
  | :
  | x  C
  | |
  | o  B
  |/
  o  A
  

The feature can be turned off:

  $ hg sl -T '{desc}' --config smartlog.collapse-obsolete=false
  o  F
  |
  o  E
  |
  o  D
  |
  o  C
  |
  o  Z
  |
  | o  G
  | |
  | x  F
  | |
  | x  E
  | |
  | x  D
  | |
  | x  C
  | |
  | o  B
  |/
  o  A
  
