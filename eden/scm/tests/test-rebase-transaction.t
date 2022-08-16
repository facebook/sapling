#chg-compatible
#debugruntest-compatible

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
  $ hg rebase --debug -b D -d Z 2>&1 | grep 'status stored'
  rebase status stored
  rebase status stored
  $ tglog
  o  a701fddfacec 'D' D
  │
  o  abc67d0cf023 'C' C
  │
  o  9a6b5541d0c0 'B' B
  │
  o  e9b22a392ce0 'Z' Z
  │
  o  633ae0eca5f4 'Y' Y
  │
  o  426bada5c675 'A' A
  
  $ cd ..
