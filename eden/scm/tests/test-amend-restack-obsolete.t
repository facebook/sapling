#chg-compatible
#debugruntest-compatible

  $ configure mutation-norecord
  $ enable amend rebase
  $ setconfig rebase.experimental.inmemory=True
  $ setconfig rebase.singletransaction=True
  $ setconfig amend.autorestack=no-conflict
  $ mkcommit() {
  >   echo "$1" > "$1"
  >   hg add "$1"
  >   hg ci -m "add $1"
  > }

Test invalid value for amend.autorestack
  $ newrepo
  $ hg debugdrawdag<<'EOS'
  >    D
  >    |
  > C  C_old
  > |  |      # amend: B_old -> B
  > B  B_old  # amend: C_old -> C
  > | /
  > |/
  > A
  > EOS
  $ hg goto -qC B
  $ echo "new content" > B
  $ showgraph
  o  3c36beb5705f D
  │
  │ o  26805aba1e60 C
  │ │
  x │  07863d11c289 C_old
  │ │
  │ @  112478962961 B
  │ │
  x │  3326d5194fc9 B_old
  ├─╯
  o  426bada5c675 A
  $ hg amend -m "B'"
  restacking children automatically (unless they conflict)
  rebasing 26805aba1e60 "C" (C)
  $ showgraph
  o  5676eb48a524 C
  │
  @  180681c3ccd0 B'
  │
  │ o  3c36beb5705f D
  │ │
  │ x  07863d11c289 C_old
  │ │
  │ x  3326d5194fc9 B_old
  ├─╯
  o  426bada5c675 A
  $ hg rebase --restack
  rebasing 3c36beb5705f "D" (D)
  $ showgraph
  o  d1e904d06977 D
  │
  o  5676eb48a524 C
  │
  @  180681c3ccd0 B'
  │
  o  426bada5c675 A
