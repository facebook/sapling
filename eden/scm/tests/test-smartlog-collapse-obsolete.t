#chg-compatible
#debugruntest-compatible

  $ configure modern
  $ setconfig format.use-segmented-changelog=1
  $ enable smartlog rebase
  $ disable commitcloud

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
  │
  o  E
  │
  o  D
  │
  o  C
  │
  o  Z
  │
  │ o  G
  │ │
  │ x  F
  │ ╷
  │ x  C
  │ │
  │ o  B
  ├─╯
  o  A
  

The feature can be turned off:

  $ hg sl -T '{desc}' --config smartlog.collapse-obsolete=false
  o  F
  │
  o  E
  │
  o  D
  │
  o  C
  │
  o  Z
  │
  │ o  G
  │ │
  │ x  F
  │ │
  │ x  E
  │ │
  │ x  D
  │ │
  │ x  C
  │ │
  │ o  B
  ├─╯
  o  A
  

The "." is always shown using the default command:

  $ hg up -q 'min(desc(D))'
  $ hg sl -T '{desc}' --config smartlog.collapse-obsolete=true
  o  F
  │
  o  E
  │
  o  D
  │
  o  C
  │
  o  Z
  │
  │ o  G
  │ │
  │ x  F
  │ ╷
  │ @  D
  │ │
  │ x  C
  │ │
  │ o  B
  ├─╯
  o  A
  

"." can still be hidden or shown with explicit `-r`:

  $ hg sl -T '{desc}' -r 'desc(G)' --config smartlog.collapse-obsolete=true
  o  Z
  │
  │ o  G
  │ │
  │ x  F
  │ ╷
  │ x  C
  │ │
  │ o  B
  ├─╯
  o  A
  

  $ hg sl -T '{desc}' -r 'desc(G)+.' --config smartlog.collapse-obsolete=true
  o  Z
  │
  │ o  G
  │ │
  │ x  F
  │ ╷
  │ @  D
  │ │
  │ x  C
  │ │
  │ o  B
  ├─╯
  o  A
  
"-r" with obsoleted stack.

  $ hg hide -q 'desc(G)'
  $ hg up -q 'desc(Z)'
  $ hg sl -T '{desc}' -r 'desc(F) - (desc(Z)::)'
  @  Z
  │
  │ o  F
  │ │
  │ o  E
  │ │
  │ o  D
  │ │
  │ o  C
  │ │
  │ o  B
  ├─╯
  o  A
  
  $ hg sl -T '{desc}' -r 'desc(F) - (desc(Z)::)' --config smartlog.collapse-obsolete=false
  @  Z
  │
  │ x  F
  │ │
  │ x  E
  │ │
  │ x  D
  │ │
  │ x  C
  │ │
  │ o  B
  ├─╯
  o  A
  
