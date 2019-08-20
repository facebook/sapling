Set up test environment.
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > amend=
  > rebase=
  > [experimental]
  > evolution = createmarkers, allowunstable
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

Cannot --rebase and --merge.
  $ hg next --rebase --merge
  abort: cannot use both --merge and --rebase
  [255]

Build dag with instablility
  $ hg debugbuilddag -n +4
  $ hg up 1
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg amend -m "amended" --no-rebase
  hint[amend-restack]: descendants of e8ec16b776b6 are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints

Check the next behaviour in case of ambiguity between obsolete and non-obsolete
  $ showgraph
  @  4 amended
  |
  | o  3 r3
  | |
  | o  2 r2
  | |
  | x  1 r1
  |/
  o  0 r0
  
  $ hg prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [612462] r0
  $ hg next
  changeset 61246295ee1e has multiple children, namely:
  [e8ec16] r1
  [dc00ac] amended
  choosing the only non-obsolete child: dc00accb61d3
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [dc00ac] amended

Rebasing single changeset.
  $ hg next
  abort: current changeset has no children
  [255]
  $ hg next --rebase
  rebasing 2:776c07fa2b12 "r2"
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [fe8ffc] r2
  $ showgraph
  @  5 r2
  |
  o  4 amended
  |
  | o  3 r3
  | |
  | x  2 r2
  | |
  | x  1 r1
  |/
  o  0 r0
  
Test --clean flag.
  $ touch foo
  $ hg add foo
  $ hg status
  A foo
  $ hg next --rebase
  abort: uncommitted changes
  (use --clean to discard uncommitted changes or --merge to bring them along)
  [255]
  $ hg next --rebase --clean
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  rebasing 3:137d867d71d5 "r3"
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [7d603c] r3
  $ hg status
  ? foo
  $ showgraph
  @  6 r3
  |
  o  5 r2
  |
  o  4 amended
  |
  o  0 r0
  

Rebasing multiple changesets at once.
  $ reset
  $ hg debugbuilddag -n +5
  $ hg up 1
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg amend -m "amended" --no-rebase
  hint[amend-restack]: descendants of e8ec16b776b6 are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints
  $ hg next --rebase --top
  rebasing 2:776c07fa2b12 "r2"
  rebasing 3:137d867d71d5 "r3"
  rebasing 4:daa37004f338 "r4"
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [55b98e] r4
  $ showgraph
  @  8 r4
  |
  o  7 r3
  |
  o  6 r2
  |
  o  5 amended
  |
  o  0 r0
  

Rebasing a stack one changeset at a time.
  $ reset
  $ hg debugbuilddag -n +5
  $ hg up 1
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg amend -m "amended" --no-rebase
  hint[amend-restack]: descendants of e8ec16b776b6 are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints
  $ hg next --rebase
  rebasing 2:776c07fa2b12 "r2"
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [fe8ffc] r2
  $ hg next --rebase
  rebasing 3:137d867d71d5 "r3"
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [7d603c] r3
  $ showgraph
  @  7 r3
  |
  o  6 r2
  |
  o  5 amended
  |
  | o  4 r4
  | |
  | x  3 r3
  | |
  | x  2 r2
  | |
  | x  1 r1
  |/
  o  0 r0
  

  $ hg next --rebase
  rebasing 4:daa37004f338 "r4"
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [55b98e] r4
  $ showgraph
  @  8 r4
  |
  o  7 r3
  |
  o  6 r2
  |
  o  5 amended
  |
  o  0 r0
  

Rebasing a stack two changesets at a time.
  $ reset
  $ hg debugbuilddag -n +6
  $ hg up 1
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg amend -m "amended" --no-rebase
  hint[amend-restack]: descendants of e8ec16b776b6 are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints
  $ hg next --rebase 2
  rebasing 2:776c07fa2b12 "r2"
  rebasing 3:137d867d71d5 "r3"
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [7d603c] r3
  $ showgraph
  @  8 r3
  |
  o  7 r2
  |
  o  6 amended
  |
  | o  5 r5
  | |
  | o  4 r4
  | |
  | x  3 r3
  | |
  | x  2 r2
  | |
  | x  1 r1
  |/
  o  0 r0
  
  $ hg next --rebase 2
  rebasing 4:daa37004f338 "r4"
  rebasing 5:5f333e6f7274 "r5"
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [c01a35] r5
  $ showgraph
  @  10 r5
  |
  o  9 r4
  |
  o  8 r3
  |
  o  7 r2
  |
  o  6 amended
  |
  o  0 r0
  

Rebasing after multiple amends.
  $ reset
  $ hg debugbuilddag -n +5
  $ hg up 1
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg amend -m "amend 1" --no-rebase
  hint[amend-restack]: descendants of e8ec16b776b6 are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints
  $ hg amend -m "amend 2"
  $ hg amend -m "amend 3"
  $ showgraph
  @  7 amend 3
  |
  | o  4 r4
  | |
  | o  3 r3
  | |
  | o  2 r2
  | |
  | x  1 r1
  |/
  o  0 r0
  
  $ hg next --rebase --top
  rebasing 2:776c07fa2b12 "r2"
  rebasing 3:137d867d71d5 "r3"
  rebasing 4:daa37004f338 "r4"
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [66772e] r4
  $ showgraph
  @  10 r4
  |
  o  9 r3
  |
  o  8 r2
  |
  o  7 amend 3
  |
  o  0 r0
  

Rebasing from below the amended changeset with the --newest flag.
  $ reset
  $ hg debugbuilddag -n +6
  $ hg up 2
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg amend -m "amended" --no-rebase
  hint[amend-restack]: descendants of 776c07fa2b12 are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints
  $ hg up 0
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ showgraph
  o  6 amended
  |
  | o  5 r5
  | |
  | o  4 r4
  | |
  | o  3 r3
  | |
  | x  2 r2
  |/
  o  1 r1
  |
  @  0 r0
  
  $ hg next --rebase --top --newest
  rebasing 3:137d867d71d5 "r3"
  rebasing 4:daa37004f338 "r4"
  rebasing 5:5f333e6f7274 "r5"
  5 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [bd05f8] r5
  $ showgraph
  @  9 r5
  |
  o  8 r4
  |
  o  7 r3
  |
  o  6 amended
  |
  o  1 r1
  |
  o  0 r0
  

Test aborting due to ambiguity caused by a rebase. The rebase should be
rolled back and the final state should be as it was before `hg next --rebase`.
  $ reset
  $ hg debugbuilddag -n +6
  $ hg up 1
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg amend -m "amended" --no-rebase
  hint[amend-restack]: descendants of e8ec16b776b6 are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints
  $ mkcommit a
  $ hg prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [dc00ac] amended
  $ showgraph
  o  7 add a
  |
  @  6 amended
  |
  | o  5 r5
  | |
  | o  4 r4
  | |
  | o  3 r3
  | |
  | o  2 r2
  | |
  | x  1 r1
  |/
  o  0 r0
  
  $ hg next --rebase
  rebasing 2:776c07fa2b12 "r2"
  changeset dc00accb61d3 has multiple children, namely:
  [4e13d3] add a
  [fe8ffc] r2
  transaction abort!
  rollback completed
  abort: ambiguous next changeset
  (use the --newest or --towards flags to specify which child to pick)
  [255]
  $ showgraph
  o  7 add a
  |
  @  6 amended
  |
  | o  5 r5
  | |
  | o  4 r4
  | |
  | o  3 r3
  | |
  | o  2 r2
  | |
  | x  1 r1
  |/
  o  0 r0
  

Test a situation where there is a conflict.
  $ reset
  $ mkcommit a
  $ mkcommit b
  $ mkcommit c
  $ mkcommit d
  $ hg up 1
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ echo "conflict" > c
  $ hg add c
  $ hg amend -m "amended to add c" --no-rebase
  hint[amend-restack]: descendants of 7c3bad9141dc are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints
  $ showgraph
  @  4 amended to add c
  |
  | o  3 add d
  | |
  | o  2 add c
  | |
  | x  1 add b
  |/
  o  0 add a
  
  $ hg next --rebase --top
  rebasing 2:4538525df7e2 "add c"
  merging c
  warning: 1 conflicts while merging c! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ showgraph
  @  4 amended to add c
  |
  | o  3 add d
  | |
  | @  2 add c
  | |
  | x  1 add b
  |/
  o  0 add a
  
In this mid-rebase state, we can't use `hg previous` or `hg next`:
  $ hg previous
  abort: rebase in progress
  (use 'hg rebase --continue' or 'hg rebase --abort')
  [255]
Now resolve the conflict and resume the rebase.
  $ rm c
  $ echo "resolved" > c
  $ hg resolve --mark c
  (no more unresolved files)
  continue: hg rebase --continue
  $ hg rebase --continue
  rebasing 2:4538525df7e2 "add c"
  $ showgraph
  o  5 add c
  |
  @  4 amended to add c
  |
  | o  3 add d
  | |
  | x  2 add c
  | |
  | x  1 add b
  |/
  o  0 add a
  
