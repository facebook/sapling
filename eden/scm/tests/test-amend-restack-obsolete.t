#chg-compatible

  $ . helpers-usechg.sh
  $ enable mutation-norecord amend rebase
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
  $ hg update -qC B
  $ echo "new content" > B
  $ showgraph
  o  5 3c36beb5705f D
  |
  | o  4 26805aba1e60 C
  | |
  x |  3 07863d11c289 C_old
  | |
  | @  2 112478962961 B
  | |
  x |  1 3326d5194fc9 B_old
  |/
  o  0 426bada5c675 A
  $ hg amend -m "B'"
  restacking children automatically (unless they conflict)
  rebasing 26805aba1e60 "C" (C)
  $ showgraph
  o  7 5676eb48a524 C
  |
  @  6 180681c3ccd0 B'
  |
  | o  5 3c36beb5705f D
  | |
  | x  3 07863d11c289 C_old
  | |
  | x  1 3326d5194fc9 B_old
  |/
  o  0 426bada5c675 A
  $ hg rebase --restack
  rebasing 3c36beb5705f "D" (D)
  $ showgraph
  o  8 d1e904d06977 D
  |
  o  7 5676eb48a524 C
  |
  @  6 180681c3ccd0 B'
  |
  o  0 426bada5c675 A
