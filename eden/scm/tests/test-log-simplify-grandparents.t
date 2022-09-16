#debugruntest-compatible
  $ newrepo
  $ drawdag << 'EOS'
  > E
  > |
  > D
  > |\
  > B C
  > |/
  > A
  > EOS
  $ hg bookmark -r $A v1
  $ hg bookmark -r $B v2
  $ hg bookmark -r $E v3
  $ hg debugmakepublic -r $E

With simplify-grandparents disabled:

  $ setconfig log.simplify-grandparents=0

  $ hg smartlog -T '{desc} {bookmarks}' --config extensions.smartlog=
  o    E v3
  ├─╮
  ╷ o  B v2
  ╭─╯
  o  A v1
  
  $ hg log -Gr 'bookmark()' -T '{desc} {bookmarks}'
  o    E v3
  ├─╮
  ╷ o  B v2
  ╭─╯
  o  A v1
  
With simplify-grandparents enabled:

  $ setconfig log.simplify-grandparents=1

  $ hg smartlog -T '{desc} {bookmarks}' --config extensions.smartlog=
  o  E v3
  ╷
  o  B v2
  │
  o  A v1
  

  $ hg log -Gr 'bookmark()' -T '{desc} {bookmarks}'
  o  E v3
  ╷
  o  B v2
  │
  o  A v1
  
