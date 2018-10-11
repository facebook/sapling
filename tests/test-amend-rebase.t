Set up test environment.
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > amend=
  > rebase=
  > [experimental]
  > evolution = createmarkers, allowunstable
  > EOF

Test that rebased commits that would cause instability are inhibited.
  $ hg init repo && cd repo
  $ hg debugbuilddag -m '+3 *3'
  $ showgraph
  o  3 e5d56d7a7894 r3
  |
  | o  2 c175bafe34cb r2
  | |
  | o  1 22094967a90d r1
  |/
  o  0 1ad88bca4140 r0
  $ hg rebase -r 1 -d 3
  rebasing 1:* "r1" (glob)
  merging mf
  $ showgraph
  o  4 309a29d7f33b r1
  |
  o  3 e5d56d7a7894 r3
  |
  | o  2 c175bafe34cb r2
  | |
  | x  1 22094967a90d r1
  |/
  o  0 1ad88bca4140 r0
