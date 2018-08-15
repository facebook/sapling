Set up test environment.
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > fbamend=
  > rebase=
  > [experimental]
  > evolution = createmarkers, allowunstable
  > EOF

Test that rebased commits that would cause instability are inhibited.
  $ hg init repo && cd repo
  $ hg debugbuilddag -m '+3 *3'
  $ showgraph
  o  3 r3
  |
  | o  2 r2
  | |
  | o  1 r1
  |/
  o  0 r0
  $ hg rebase -r 1 -d 3
  rebasing 1:* "r1" (glob)
  merging mf
  $ showgraph
  o  4 r1
  |
  o  3 r3
  |
  | o  2 r2
  | |
  | x  1 r1
  |/
  o  0 r0
