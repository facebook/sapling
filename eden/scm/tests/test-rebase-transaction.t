#chg-compatible

  $ enable rebase
  $ setconfig phases.publish=false

Rebasing using a single transaction

  $ hg init singletr && cd singletr
  $ setconfig rebase.singletransaction=true
  $ hg debugdrawdag <<'EOF'
  >   Z
  >   |
  >   | D
  >   | |
  >   | C
  >   | |
  >   Y B
  >   |/
  >   A
  > EOF
- We should only see two status stored messages. One from the start, one from
- the end.
  $ hg rebase --debug -b D -d Z | grep 'status stored'
  rebase status stored
  rebase status stored
  $ tglog
  o  8: a701fddfacec 'D' D
  |
  o  7: abc67d0cf023 'C' C
  |
  o  6: 9a6b5541d0c0 'B' B
  |
  o  4: e9b22a392ce0 'Z' Z
  |
  o  2: 633ae0eca5f4 'Y' Y
  |
  o  0: 426bada5c675 'A' A
  
  $ cd ..
