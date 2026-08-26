
#require no-eden


  $ configure modern
  $ enable smartlog rebase
  $ disable commitcloud
  $ setconfig commit.modify-obsolete-mode=ignore

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
  $ sl bookmark -r $Z master
  $ sl bookmark -r $G old
  $ sl bookmark -r $F new
  $ sl rebase -qr $C::$F -d $Z

The obsoleted C::F should be collapsed:

  $ sl sl -T '{desc}' --config smartlog.collapse-obsolete=true
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

  $ sl sl -T '{desc}' --config smartlog.collapse-obsolete=false
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

  $ sl up -q 'min(desc(D))'
  $ sl sl -T '{desc}' --config smartlog.collapse-obsolete=true
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

  $ sl sl -T '{desc}' -r 'desc(G)' --config smartlog.collapse-obsolete=true
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
  

  $ sl sl -T '{desc}' -r 'desc(G)+.' --config smartlog.collapse-obsolete=true
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

  $ sl hide -q 'desc(G)'
  $ sl up -q 'desc(Z)'
  $ sl sl -T '{desc}' -r 'desc(F) - (desc(Z)::)'
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
  
  $ sl sl -T '{desc}' -r 'desc(F) - (desc(Z)::)' --config smartlog.collapse-obsolete=false
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
  
