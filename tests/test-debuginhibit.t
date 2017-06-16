Set up test environment.
  $ extpath=`dirname $TESTDIR`
  $ cp $extpath/hgext3rd/debuginhibit.py $TESTTMP
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > debuginhibit=$TESTTMP/debuginhibit.py
  > directaccess=$TESTDIR/../hgext3rd/directaccess.py
  > fbamend=$TESTDIR/../hgext3rd/fbamend
  > inhibit=$TESTDIR/../hgext3rd/inhibit.py
  > [debuginhibit]
  > printnodes = true
  > printstack = true
  > stackdepth = 1
  > [experimental]
  > evolution = createmarkers
  > EOF
  $ showgraph() {
  >   hg log --graph -T "{rev}:{node|short} {desc|firstline}"
  > }

Test manually inhibiting and deinhibiting nodes.
  $ hg init allowunstable && cd allowunstable
  $ hg debugbuilddag "+3 *3"
  $ showgraph
  o  3:6100d3090acf r3
  |
  | o  2:01241442b3c2 r2
  | |
  | o  1:66f7d451a68b r1
  |/
  o  0:1ea73414a91b r0
  
  $ hg debugobsolete 66f7d451a68b85ed82ff5fcc254daf50c74144bd 6100d3090acf50ed11ec23196cec20f5bd7323aa --config "debuginhibit.printstack=false"
  Inhibiting: ['66f7d451a68b']
  $ hg log -r 'unstable()'
  $ hg debuginhibit
  1:66f7d451a68b85ed82ff5fcc254daf50c74144bd r1
  $ hg debuginhibit -d 1
  Deinhibiting: ['66f7d451a68b']
  Context:
  	[debuginhibit.py:*] debuginhibit() (glob)
  $ hg log -r 'unstable()'
  changeset:   2:01241442b3c2
  user:        debugbuilddag
  date:        Thu Jan 01 00:00:02 1970 +0000
  trouble:     unstable
  summary:     r2
  
  $ hg debuginhibit
  $ hg debuginhibit 1
  Inhibiting: ['66f7d451a68b']
  Context:
  	[debuginhibit.py:*] debuginhibit() (glob)
