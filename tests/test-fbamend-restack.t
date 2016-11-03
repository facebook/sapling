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
  

Test finding a stable base commit from within the old stack.
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
  $ hg up 3
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ showgraph
  o  5 add b
  |
  | @  3 add d
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
  @  7 add d
  |
  o  6 add c
  |
  o  5 add b
  |
  o  0 add a
  

Test finding a stable base commit from a new child of the amended commit.
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
  $ mkcommit e
  $ showgraph
  @  6 add e
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
  
  $ hg rebase --restack
  rebasing 2:4538525df7e2 "add c"
  rebasing 3:47d2a3944de8 "add d"
  $ showgraph
  o  8 add d
  |
  o  7 add c
  |
  | @  6 add e
  |/
  o  5 add b
  |
  o  0 add a
  

Test finding a stable base commit when there are multiple amends and
a commit on top of one of the obsolete intermediate commits.
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
  $ mkcommit e
  $ hg prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [*] add b (glob)
  $ echo b >> b
  $ hg amend
  warning: the changeset's children were left behind
  (use 'hg amend --fixup' to rebase them)
  $ hg up 6
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ showgraph
  o  8 add b
  |
  | @  6 add e
  | |
  | o  5 add b
  |/
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
  rebasing 6:c1992d8998fa "add e"
  $ showgraph
  @  11 add e
  |
  | o  10 add d
  | |
  | o  9 add c
  |/
  o  8 add b
  |
  o  0 add a
  

Test that we only use the closest stable base commit.
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
  $ hg up 2
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo c >> c
  $ hg amend
  warning: the changeset's children were left behind
  (use 'hg amend --fixup' to rebase them)
  $ hg up 3
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ showgraph
  o  7 add c
  |
  | o  5 add b
  | |
  | | @  3 add d
  | | |
  +---o  2 add c
  | |
  o |  1 add b
  |/
  o  0 add a
  
  $ hg rebase --restack
  rebasing 3:47d2a3944de8 "add d"
  $ showgraph
  @  8 add d
  |
  o  7 add c
  |
  | o  5 add b
  | |
  o |  1 add b
  |/
  o  0 add a
  

Test what happens if there is no base commit found. The command should
fix up everything above the current commit, leaving other commits
below the current commit alone.
  $ reset
  $ mkcommit a
  $ mkcommit b
  $ mkcommit c
  $ mkcommit d
  $ mkcommit e
  $ hg up 3
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo d >> d
  $ hg amend
  warning: the changeset's children were left behind
  (use 'hg amend --fixup' to rebase them)
  $ hg up 0
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved
  $ mkcommit f
  created new head
  $ hg up 1
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ showgraph
  o  7 add f
  |
  | o  6 add d
  | |
  | | o  4 add e
  | | |
  | | o  3 add d
  | |/
  | o  2 add c
  | |
  | @  1 add b
  |/
  o  0 add a
  
  $ hg rebase --restack
  rebasing 4:9d206ffc875e "add e"
  $ showgraph
  o  8 add e
  |
  | o  7 add f
  | |
  o |  6 add d
  | |
  o |  2 add c
  | |
  @ |  1 add b
  |/
  o  0 add a
  

Test having an unamended commit.
  $ reset
  $ mkcommit a
  $ mkcommit b
  $ mkcommit c
  $ hg prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [*] add b (glob)
  $ echo b >> b
  $ hg amend -m "Amended"
  warning: the changeset's children were left behind
  (use 'hg amend --fixup' to rebase them)
  $ echo b >> b
  $ hg amend -m "Unamended"
  $ hg unamend
  $ hg up -C 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ showgraph
  o  4 Amended
  |
  | o  2 add c
  | |
  | @  1 add b
  |/
  o  0 add a
  
  $ hg rebase --restack
  rebasing 2:4538525df7e2 "add c"
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ showgraph
  o  7 add c
  |
  @  4 Amended
  |
  o  0 add a
  

Test situation with divergence.
  $ reset
  $ mkcommit a
  $ mkcommit b
  $ mkcommit c
  $ hg prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [*] add b (glob)
  $ echo b >> b
  $ hg amend
  warning: the changeset's children were left behind
  (use 'hg amend --fixup' to rebase them)
  $ hg up 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo c >> b
  $ hg amend
  warning: the changeset's children were left behind
  (use 'hg amend --fixup' to rebase them)
  $ hg up 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ showgraph
  o  6 add b
  |
  | o  4 add b
  |/
  | o  2 add c
  | |
  | @  1 add b
  |/
  o  0 add a
  
  $ hg rebase --restack
  abort: changeset 7c3bad9141dcb46ff89abf5f61856facd56e476c has multiple newer versions, cannot automatically determine latest verion
  [255]

Test situation with divergence due to an unamend. This should actually succeed
since the successor is obsolete.
  $ reset
  $ mkcommit a
  $ mkcommit b
  $ mkcommit c
  $ hg prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [*] add b (glob)
  $ echo b >> b
  $ hg amend
  warning: the changeset's children were left behind
  (use 'hg amend --fixup' to rebase them)
  $ hg up 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo c >> b
  $ hg amend
  warning: the changeset's children were left behind
  (use 'hg amend --fixup' to rebase them)
  $ hg unamend
  $ hg up -C 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ showgraph
  o  4 add b
  |
  | o  2 add c
  | |
  | @  1 add b
  |/
  o  0 add a
  
  $ hg rebase --restack
  rebasing 2:4538525df7e2 "add c"
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ showgraph
  o  7 add c
  |
  @  4 add b
  |
  o  0 add a
  

Test recursive restacking -- basic case.
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
  $ hg up 2
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo c >> c
  $ hg amend
  warning: the changeset's children were left behind
  (use 'hg amend --fixup' to rebase them)
  $ hg up 1
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ showgraph
  o  7 add c
  |
  | o  5 add b
  | |
  | | o  3 add d
  | | |
  +---o  2 add c
  | |
  @ |  1 add b
  |/
  o  0 add a
  
  $ hg rebase --restack
  rebasing 3:47d2a3944de8 "add d"
  rebasing 7:a43fcd08f41f "add c" (tip)
  rebasing 8:49b119a57122 "add d"
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ showgraph
  o  10 add d
  |
  o  9 add c
  |
  @  5 add b
  |
  o  0 add a
  

Test recursive restacking -- more complex case. This test is designed to
to check for a bug encountered if rebasing is performed naively from the
bottom-up wherein obsolescence information for commits further up the
stack is lost upon rebasing lower levels.
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
  $ mkcommit e
  $ mkcommit f
  $ hg prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [*] add e (glob)
  $ echo e >> e
  $ hg amend
  warning: the changeset's children were left behind
  (use 'hg amend --fixup' to rebase them)
  $ hg up 2
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo c >> c
  $ hg amend
  warning: the changeset's children were left behind
  (use 'hg amend --fixup' to rebase them)
  $ mkcommit g
  $ mkcommit h
  $ hg prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [*] add g (glob)
  $ echo g >> g
  $ hg amend
  warning: the changeset's children were left behind
  (use 'hg amend --fixup' to rebase them)
  $ hg up 1
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ showgraph
  o  15 add g
  |
  | o  13 add h
  | |
  | o  12 add g
  |/
  o  11 add c
  |
  | o  9 add e
  | |
  | | o  7 add f
  | | |
  | | o  6 add e
  | |/
  | o  5 add b
  | |
  | | o  3 add d
  | | |
  +---o  2 add c
  | |
  @ |  1 add b
  |/
  o  0 add a
  
  $ hg rebase --restack
  rebasing 13:9f2a7cefd4b4 "add h"
  rebasing 3:47d2a3944de8 "add d"
  rebasing 7:2a79e3a98cd6 "add f"
  rebasing 11:a43fcd08f41f "add c"
  rebasing 15:604f34a1983d "add g" (tip)
  rebasing 16:e1df23499b99 "add h"
  rebasing 17:49b119a57122 "add d"
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ showgraph
  o  22 add d
  |
  | o  21 add h
  | |
  | o  20 add g
  |/
  o  19 add c
  |
  | o  18 add f
  | |
  | o  9 add e
  |/
  @  5 add b
  |
  o  0 add a
  
