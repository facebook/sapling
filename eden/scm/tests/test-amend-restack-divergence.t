#chg-compatible
#debugruntest-compatible


  $ configure mutation-norecord
  $ enable amend rebase
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
  $ hg up 7c3bad9141dcb46ff89abf5f61856facd56e476c
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg amend -m "successor 2" --no-rebase
  hint[amend-restack]: descendants of 7c3bad9141dc are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints
  $ hg up 7c3bad9141dcb46ff89abf5f61856facd56e476c
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ showgraph
  o  cef323f40828 successor 2
  │
  │ o  f60c1f15a70e successor 1
  ├─╯
  │ o  4538525df7e2 add c
  │ │
  │ @  7c3bad9141dc add b
  ├─╯
  o  1f0dee641bb7 add a
  $ hg rebase --restack
  rebasing 4538525df7e2 "add c"
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ showgraph
  o  b0a0bc953ac3 add c
  │
  @  cef323f40828 successor 2
  │
  │ o  f60c1f15a70e successor 1
  ├─╯
  o  1f0dee641bb7 add a

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
  @  c54ee8acf83d add b
  │
  │ o  4538525df7e2 add c
  │ │
  │ x  7c3bad9141dc add b
  ├─╯
  o  1f0dee641bb7 add a
  $ hg up 7c3bad9141dcb46ff89abf5f61856facd56e476c
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo c >> b
  $ hg amend
  hint[amend-restack]: descendants of 7c3bad9141dc are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints
  $ showgraph
  @  2c965323ca2a add b
  │
  │ o  c54ee8acf83d add b
  ├─╯
  │ o  4538525df7e2 add c
  │ │
  │ x  7c3bad9141dc add b
  ├─╯
  o  1f0dee641bb7 add a
  $ hg unamend
  $ hg up -C c54ee8acf83d47ec674bca5bb6ba7be56227bd89
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ showgraph
  @  c54ee8acf83d add b
  │
  │ o  4538525df7e2 add c
  │ │
  │ x  7c3bad9141dc add b
  ├─╯
  o  1f0dee641bb7 add a
