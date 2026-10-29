
#require no-eden

  $ eagerepo
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
  $ sl bookmark -r $A v1
  $ sl bookmark -r $B v2
  $ sl bookmark -r $E v3
  $ sl debugmakepublic -r $E

With simplify-grandparents disabled:

  $ setconfig log.simplify-grandparents=0

  $ sl smartlog -T '{desc} {bookmarks}' --config extensions.smartlog=
  o    E v3
  ├─╮
  ╷ ╷
  ╷ o  B v2
  ╭─╯
  o  A v1
  $ sl log -Gr 'bookmark()' -T '{desc} {bookmarks}'
  o    E v3
  ├─╮
  ╷ ╷
  ╷ o  B v2
  ╭─╯
  o  A v1
With simplify-grandparents enabled:

  $ setconfig log.simplify-grandparents=1

  $ sl smartlog -T '{desc} {bookmarks}' --config extensions.smartlog=
  o  E v3
  ╷
  o  B v2
  │
  o  A v1
  

  $ sl log -Gr 'bookmark()' -T '{desc} {bookmarks}'
  o  E v3
  ╷
  o  B v2
  │
  o  A v1
  
