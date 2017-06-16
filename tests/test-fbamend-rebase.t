Set up test environment.
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > debuginhibit=$TESTDIR/../hgext3rd/debuginhibit.py
  > directaccess=$TESTDIR/../hgext3rd/directaccess.py
  > fbamend=$TESTDIR/../hgext3rd/fbamend
  > inhibit=$TESTDIR/../hgext3rd/inhibit.py
  > rebase=
  > [experimental]
  > evolution = createmarkers, allowunstable
  > EOF
  $ showgraph() {
  >   hg log --graph -T "{rev} {desc|firstline}" | sed \$d
  > }

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
  $ hg rebase -r 1 -d 3 --config "debuginhibit.printnodes=true"
  rebasing 1:* "r1" (glob)
  merging mf
  Inhibiting: ['*'] (glob)
  Deinhibiting: ['*'] (glob)
  Deinhibiting: []
  Inhibiting: ['*'] (glob)
  $ showgraph
  o  4 r1
  |
  o  3 r3
  |
  | o  2 r2
  | |
  | o  1 r1
  |/
  o  0 r0
Make sure there are no unstable commits.
  $ hg log -r 'unstable()'
