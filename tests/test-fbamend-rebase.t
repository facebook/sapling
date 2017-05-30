Set up test environment.
  $ . $TESTDIR/require-ext.sh evolve
  $ extpath=`dirname $TESTDIR`
  $ cp $extpath/hgext3rd/allowunstable.py $TESTTMP
  $ cp $extpath/hgext3rd/debuginhibit.py $TESTTMP
  $ cp $extpath/hgext3rd/fbamend.py $TESTTMP
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > allowunstable=$TESTTMP/allowunstable.py
  > debuginhibit=$TESTTMP/debuginhibit.py
  > directaccess=$TESTDIR/../hgext3rd/directaccess.py
  > evolve=
  > fbamend=$TESTTMP/fbamend.py
  > inhibit=$TESTDIR/../hgext3rd/inhibit.py
  > rebase=
  > [experimental]
  > evolution = createmarkers
  > evolutioncommands = prev next fold split
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
