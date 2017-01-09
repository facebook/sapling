Set up test environment.
  $ . $TESTDIR/require-ext.sh directaccess evolve inhibit
  $ extpath=`dirname $TESTDIR`
  $ cp $extpath/hgext3rd/fbamend.py $TESTTMP # use $TESTTMP substitution in message
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > directaccess=
  > evolve=
  > fbamend=$TESTTMP/fbamend.py
  > inhibit=
  > rebase=
  > strip=
  > [experimental]
  > evolution = createmarkers
  > evolutioncommands = prev next
  > [fbamend]
  > userestack=true
  > EOF
  $ mkcommit() {
  >   echo "$1" > "$1"
  >   hg add "$1"
  >   echo "add $1" > msg
  >   hg ci -l msg
  > }
  $ reset() {
  >   cd ..
  >   rm -rf userestack
  >   hg init userestack
  >   cd userestack
  > }
  $ showgraph() {
  >   hg log --graph -T "{rev} {desc|firstline}"
  > }
  $ hg init userestack && cd userestack

Test that no preamend bookmark is created.
  $ mkcommit a
  $ mkcommit b
  $ mkcommit c
  $ mkcommit d
  $ hg up 1
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg amend -m "amended"
  warning: the changeset's children were left behind
  (use 'hg rebase --restack' (alias: 'hg restack') to rebase them)
  $ hg book
  no bookmarks set

Test hg amend --fixup.
  $ showgraph
  @  4 amended
  |
  | o  3 add d
  | |
  | o  2 add c
  | |
  | o  1 add b
  |/
  o  0 add a
  
  $ hg amend --fixup
  rebasing 2:4538525df7e2 "add c"
  rebasing 3:47d2a3944de8 "add d"
  $ showgraph
  o  6 add d
  |
  o  5 add c
  |
  @  4 amended
  |
  o  0 add a
  

Test hg amend --rebase
  $ hg amend -m "amended again" --rebase
  rebasing 5:89333fbce6cd "add c"
  rebasing 6:33b3e8631c12 "add d"
  $ showgraph
  o  9 add d
  |
  o  8 add c
  |
  @  7 amended again
  |
  o  0 add a
  
