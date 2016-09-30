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
  > EOF
  $ mkcommit() {
  >   echo "$1" > "$1"
  >   hg add "$1"
  >   echo "add $1" > msg
  >   hg ci -l msg
  > }
  $ reset() {
  >   cd ..
  >   rm -rf nextrebase
  >   hg init nextrebase
  >   cd nextrebase
  > }
  $ showgraph() {
  >   hg log --graph -T "{rev} {desc|firstline}"
  > }
  $ hg init nextrebase && cd nextrebase

Ensure that the hg next --evolve is disabled.
  $ hg next --evolve
  abort: the --evolve flag is not supported
  (use 'hg next --rebase' instead)
  [255]

Check case where there's nothing to rebase.
  $ mkcommit a
  $ mkcommit b
  $ mkcommit c
  $ hg prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [1] add b
  $ hg next --rebase
  found no changesets to rebase, doing normal 'hg next' instead
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [2] add c

Create a situation where child commits are left behind after amend.
  $ hg prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [1] add b
  $ echo "b2" > b2
  $ hg add b2
  $ hg amend -m "add b and b2"
  warning: the changeset's children were left behind
  (use 'hg amend --fixup' to rebase them)
  $ showgraph
  @  4 add b and b2
  |
  | o  2 add c
  | |
  | o  1 add b
  |/
  o  0 add a
  

Check that hg rebase --next works in the simple case.
  $ hg next --rebase --dry-run
  hg rebase -r 4538525df7e2b9f09423636c61ef63a4cb872a2d -d 29509da8015c02a5a44d703e561252f6478a1430 -k
  hg next
  $ hg next --rebase
  rebasing 2:4538525df7e2 "add c"
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [5] add c
  $ showgraph
  @  5 add c
  |
  o  4 add b and b2
  |
  o  0 add a
  

Ensure we abort if there are multiple children on a precursor.
  $ reset
  $ mkcommit a
  $ mkcommit b
  $ mkcommit c
  $ hg prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [1] add b
  $ mkcommit d
  created new head
  $ hg prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [1] add b
  $ echo "b3" > b3
  $ hg add b3
  $ hg amend -m "add b and b3"
  warning: the changeset's children were left behind
  (use 'hg amend --fixup' to rebase them)
  $ showgraph
  @  5 add b and b3
  |
  | o  3 add d
  | |
  | | o  2 add c
  | |/
  | o  1 add b
  |/
  o  0 add a
  

  $ hg next --rebase
  there are multiple child changesets on previous versions of the current changeset, namely:
  [4538] add c
  [78f8] add d
  abort: ambiguous next changeset to rebase
  (please rebase the desired one manually)
  [255]

Check behavior when there is a child on the current changeset and on
a precursor.
  $ reset
  $ mkcommit a
  $ mkcommit b
  $ mkcommit c
  $ hg prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [1] add b
  $ echo b >> b
  $ hg amend
  warning: the changeset's children were left behind
  (use 'hg amend --fixup' to rebase them)
  $ mkcommit d
  $ hg prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [4] add b
  $ showgraph
  o  5 add d
  |
  @  4 add b
  |
  | o  2 add c
  | |
  | o  1 add b
  |/
  o  0 add a
  

  $ hg next --rebase
  there are child changesets on one or more previous versions of the current changeset, but the current version also has children
  skipping rebasing the following child changesets:
  [4538] add c
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [5] add d

Check the case where multiple amends have occurred.
  $ reset
  $ mkcommit a
  $ mkcommit b
  $ mkcommit c
  $ hg prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [1] add b
  $ echo b >> b
  $ hg amend
  warning: the changeset's children were left behind
  (use 'hg amend --fixup' to rebase them)
  $ echo b >> b
  $ hg amend
  $ echo b >> b
  $ hg amend
  $ showgraph
  @  8 add b
  |
  | o  2 add c
  | |
  | o  1 add b
  |/
  o  0 add a
  

  $ hg next --rebase
  rebasing 2:4538525df7e2 "add c"
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [9] add c
  $ showgraph
  @  9 add c
  |
  o  8 add b
  |
  o  0 add a
  

Check whether we can rebase a stack of commits.
  $ reset
  $ mkcommit a
  $ mkcommit b
  $ mkcommit c
  $ mkcommit d
  $ hg up 1
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ echo b >> b
  $ hg amend
  warning: the changeset's children were left behind
  (use 'hg amend --fixup' to rebase them)
  $ showgraph
  @  5 add b
  |
  | o  3 add d
  | |
  | o  2 add c
  | |
  | o  1 add b
  |/
  o  0 add a
  

  $ hg next --rebase
  rebasing 2:4538525df7e2 "add c"
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [6] add c
  $ showgraph
  @  6 add c
  |
  o  5 add b
  |
  | o  3 add d
  | |
  | o  2 add c
  | |
  | o  1 add b
  |/
  o  0 add a
  

After rebasing the last commit in the stack, the old stack should be stripped.
  $ hg next --rebase
  rebasing 3:47d2a3944de8 "add d"
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [7] add d
  $ showgraph
  @  7 add d
  |
  o  6 add c
  |
  o  5 add b
  |
  o  0 add a
  

Check whether hg next --rebase behaves correctly when there is a conflict.
  $ reset
  $ mkcommit a
  $ mkcommit b
  $ mkcommit conflict
  $ hg prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [1] add b
  $ echo "different" > conflict
  $ hg add conflict
  $ hg amend
  warning: the changeset's children were left behind
  (use 'hg amend --fixup' to rebase them)
  $ showgraph
  @  4 add b
  |
  | o  2 add conflict
  | |
  | o  1 add b
  |/
  o  0 add a
  

  $ hg next --rebase
  rebasing 2:391efaa4d81f "add conflict"
  merging conflict
  warning: conflicts while merging conflict! (edit, then use 'hg resolve --mark')
  please resolve any conflicts, run 'hg rebase --continue', and then run 'hg next'
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ hg next --rebase
  abort: rebase in progress
  (use 'hg rebase --continue' or 'hg rebase --abort')
  [255]
  $ echo "merged" > conflict
  $ hg resolve --mark conflict
  (no more unresolved files)
  continue: hg rebase --continue
  $ hg rebase --continue
  rebasing 2:391efaa4d81f "add conflict"
  $ showgraph
  o  5 add conflict
  |
  @  4 add b
  |
  | o  2 add conflict
  | |
  | o  1 add b
  |/
  o  0 add a
  
