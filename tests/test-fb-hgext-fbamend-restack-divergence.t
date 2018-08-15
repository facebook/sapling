  $ . helpers-usechg.sh

  $ enable fbamend inhibit rebase
  $ setconfig experimental.evolution.allowdivergence=True
  $ setconfig experimental.evolution="createmarkers, allowunstable"
  $ mkcommit() {
  >   echo "$1" > "$1"
  >   hg add "$1"
  >   hg ci -m "add $1"
  > }

Test situation with divergence. Restack should rebase unstable children
onto the newest successor of their parent.
  $ newrepo
  $ mkcommit a
  $ mkcommit b
  $ mkcommit c
  $ hg prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [*] add b (glob)
  $ hg amend -m "successor 1" --no-rebase
  hint[amend-restack]: descendants of 7c3bad9141dc are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints
  $ hg up 1
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg amend -m "successor 2" --no-rebase
  hint[amend-restack]: descendants of 7c3bad9141dc are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints
  $ hg up 1
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ showgraph
  o  4 successor 2
  |
  | o  3 successor 1
  |/
  | o  2 add c
  | |
  | @  1 add b
  |/
  o  0 add a
  $ hg rebase --restack
  rebasing 2:4538525df7e2 "add c"
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ showgraph
  o  5 add c
  |
  @  4 successor 2
  |
  | o  3 successor 1
  |/
  | x  1 add b
  |/
  o  0 add a

Test situation with divergence due to an unamend. This should actually succeed
since the successor is obsolete.
  $ newrepo
  $ mkcommit a
  $ mkcommit b
  $ mkcommit c
  $ hg prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [*] add b (glob)
  $ echo b >> b
  $ hg amend
  hint[amend-restack]: descendants of 7c3bad9141dc are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints
  $ showgraph
  @  3 add b
  |
  | o  2 add c
  | |
  | x  1 add b
  |/
  o  0 add a
  $ hg up 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo c >> b
  $ hg amend
  hint[amend-restack]: descendants of 7c3bad9141dc are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints
  $ showgraph
  @  4 add b
  |
  | o  3 add b
  |/
  | o  2 add c
  | |
  | x  1 add b
  |/
  o  0 add a
  $ hg unamend
  $ hg up -C 3
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ showgraph
  @  3 add b
  |
  | o  2 add c
  | |
  | o  1 add b
  |/
  o  0 add a
