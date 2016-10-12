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
  >   rm -rf rebasestack
  >   hg init rebasestack
  >   cd rebasestack
  > }
  $ showgraph() {
  >   hg log --graph -T "{rev} {desc|firstline}"
  > }
  $ hg init rebasestack && cd rebasestack

Note: Repositories populated by `hg debugbuilddag` don't seem to
correctly show all commits in the log output. Manually creating the
commits results in the expected behavior, so commits are manually
created in the test cases below.

Test unsupported flags:
  $ hg rebase --restack --rev .
  abort: cannot use both --rev and --restack
  [255]
  $ hg rebase --restack --dest .
  abort: cannot use both --dest and --restack
  [255]
  $ hg rebase --restack --source .
  abort: cannot use both --source and --restack
  [255]
  $ hg rebase --restack --base .
  abort: cannot use both --base and --restack
  [255]
  $ hg rebase --restack --abort
  abort: cannot use both --abort and --restack
  [255]
  $ hg rebase --restack --continue
  abort: cannot use both --continue and --restack
  [255]


Test basic case of a single amend in a small stack.
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
  
  $ hg rebase --restack
  rebasing 2:4538525df7e2 "add c"
  rebasing 3:47d2a3944de8 "add d"
  $ showgraph
  o  7 add d
  |
  o  6 add c
  |
  @  5 add b
  |
  o  0 add a
  

Test multiple amends of same commit.
  $ reset
  $ mkcommit a
  $ mkcommit b
  $ mkcommit c
  $ hg up 1
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ showgraph
  o  2 add c
  |
  @  1 add b
  |
  o  0 add a
  
  $ echo b >> b
  $ hg amend
  warning: the changeset's children were left behind
  (use 'hg amend --fixup' to rebase them)
  $ echo b >> b
  $ hg amend
  $ showgraph
  @  6 add b
  |
  | o  2 add c
  | |
  | o  1 add b
  |/
  o  0 add a
  

  $ hg rebase --restack
  rebasing 2:4538525df7e2 "add c"
  $ showgraph
  o  7 add c
  |
  @  6 add b
  |
  o  0 add a
  

Test conflict during rebasing.
  $ reset
  $ mkcommit a
  $ mkcommit b
  $ mkcommit c
  $ mkcommit d
  $ mkcommit e
  $ hg up 1
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved
  $ echo conflict > d
  $ hg add d
  $ hg amend
  warning: the changeset's children were left behind
  (use 'hg amend --fixup' to rebase them)
  $ showgraph
  @  6 add b
  |
  | o  4 add e
  | |
  | o  3 add d
  | |
  | o  2 add c
  | |
  | o  1 add b
  |/
  o  0 add a
  
  $ hg rebase --restack
  rebasing 2:4538525df7e2 "add c"
  rebasing 3:47d2a3944de8 "add d"
  merging d
  warning: conflicts while merging d! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ hg rebase --restack
  abort: rebase in progress
  (use 'hg rebase --continue' or 'hg rebase --abort')
  [255]
  $ echo merged > d
  $ hg resolve --mark d
  (no more unresolved files)
  continue: hg rebase --continue
  $ hg rebase --continue
  already rebased 2:4538525df7e2 "add c" as 5532778357fd
  rebasing 3:47d2a3944de8 "add d"
  rebasing 4:9d206ffc875e "add e"
  $ showgraph
  o  9 add e
  |
  o  8 add d
  |
  o  7 add c
  |
  @  6 add b
  |
  | o  1 add b
  |/
  o  0 add a
  
