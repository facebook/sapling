
  $ newclientrepo
  $ drawdag << 'EOS'
  >   A01..A20
  >  /
  > Z-B01..B20
  > C01..C20
  > EOS
  $ hg up -Cq $A20

  $ setconfig merge.max-distance=25
  $ hg merge $B20
  abort: merging distant ancestors is not supported for this repository
  (use rebase instead)
  [255]
  $ hg merge $B20 --config merge.max-distance=40
  20 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg commit -qm "merge b"

  $ hg merge $C20 --config merge.max-distance=5
  20 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg commit -qm "merge c"

  $ tglog -r 'merge() + parents(merge()) + roots(:)'
  @    b6bf8ff632b6 'merge c'
  ├─╮
  │ o    8a96775fbb08 'merge b'
  │ ├─╮
  │ │ o  ab8611c7e268 'B20'
  │ │ ╷
  │ o ╷  d022185fe2e9 'A20'
  │ ├─╯
  o ╷  079c26514042 'C20'
  ╷ ╷
  ╷ o  48b9aae0607f 'Z'
  ╷
  o  1f9ffc628a39 'C01'
