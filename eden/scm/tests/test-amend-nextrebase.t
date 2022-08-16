#chg-compatible
#debugruntest-compatible

Set up test environment.
  $ configure mutation
  $ enable amend rebase
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
  >   hg log --graph -T "{desc|firstline}"
  > }
  $ hg init nextrebase && cd nextrebase

Cannot --rebase and --merge.
  $ hg next --rebase --merge
  abort: cannot use both --merge and --rebase
  [255]

Build dag with instablility
  $ hg debugbuilddag -n +4
  $ hg up 'desc(r1)'
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg amend -m "amended" --no-rebase
  hint[amend-restack]: descendants of e8ec16b776b6 are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints

Check the next behaviour in case of ambiguity between obsolete and non-obsolete
  $ showgraph
  @  amended
  │
  │ o  r3
  │ │
  │ o  r2
  │ │
  │ x  r1
  ├─╯
  o  r0
  
  $ hg prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [612462] r0
  $ hg next
  changeset 61246295ee1e has multiple children, namely:
  [e8ec16] r1
  [f03405] amended
  choosing the only non-obsolete child: f03405deb52b
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [f03405] amended

Rebasing single changeset.
  $ hg next
  abort: current changeset has no children
  [255]
  $ hg next --rebase
  rebasing 776c07fa2b12 "r2"
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [8fb200] r2
  $ showgraph
  @  r2
  │
  o  amended
  │
  │ o  r3
  │ │
  │ x  r2
  │ │
  │ x  r1
  ├─╯
  o  r0
  
Test --clean flag.
  $ touch foo
  $ hg add foo
  $ hg status
  A foo
  $ hg next --rebase
  abort: uncommitted changes
  [255]
  $ hg next --rebase --clean
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  rebasing 137d867d71d5 "r3"
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [f12433] r3
  $ hg status
  ? foo
  $ showgraph
  @  r3
  │
  o  r2
  │
  o  amended
  │
  o  r0
  

Rebasing multiple changesets at once.
  $ reset
  $ hg debugbuilddag -n +5
  $ hg up 'desc(r1)'
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg amend -m "amended" --no-rebase
  hint[amend-restack]: descendants of e8ec16b776b6 are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints
  $ hg next --rebase --top
  rebasing 776c07fa2b12 "r2"
  rebasing 137d867d71d5 "r3"
  rebasing daa37004f338 "r4"
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [d25685] r4
  $ showgraph
  @  r4
  │
  o  r3
  │
  o  r2
  │
  o  amended
  │
  o  r0
  

Rebasing a stack one changeset at a time.
  $ reset
  $ hg debugbuilddag -n +5
  $ hg up 'desc(r1)'
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg amend -m "amended" --no-rebase
  hint[amend-restack]: descendants of e8ec16b776b6 are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints
  $ hg next --rebase
  rebasing 776c07fa2b12 "r2"
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [8fb200] r2
  $ hg next --rebase
  rebasing 137d867d71d5 "r3"
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [f12433] r3
  $ showgraph
  @  r3
  │
  o  r2
  │
  o  amended
  │
  │ o  r4
  │ │
  │ x  r3
  │ │
  │ x  r2
  │ │
  │ x  r1
  ├─╯
  o  r0
  

  $ hg next --rebase
  rebasing daa37004f338 "r4"
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [d25685] r4
  $ showgraph
  @  r4
  │
  o  r3
  │
  o  r2
  │
  o  amended
  │
  o  r0
  

Rebasing a stack two changesets at a time.
  $ reset
  $ hg debugbuilddag -n +6
  $ hg up 'desc(r1)'
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg amend -m "amended" --no-rebase
  hint[amend-restack]: descendants of e8ec16b776b6 are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints
  $ hg next --rebase 2
  rebasing 776c07fa2b12 "r2"
  rebasing 137d867d71d5 "r3"
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [f12433] r3
  $ showgraph
  @  r3
  │
  o  r2
  │
  o  amended
  │
  │ o  r5
  │ │
  │ o  r4
  │ │
  │ x  r3
  │ │
  │ x  r2
  │ │
  │ x  r1
  ├─╯
  o  r0
  
  $ hg next --rebase 2
  rebasing daa37004f338 "r4"
  rebasing 5f333e6f7274 "r5"
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [dd153e] r5
  $ showgraph
  @  r5
  │
  o  r4
  │
  o  r3
  │
  o  r2
  │
  o  amended
  │
  o  r0
  

Rebasing after multiple amends.
  $ reset
  $ hg debugbuilddag -n +5
  $ hg up 'desc(r1)'
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg amend -m "amend 1" --no-rebase
  hint[amend-restack]: descendants of e8ec16b776b6 are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints
  $ hg amend -m "amend 2"
  $ hg amend -m "amend 3"
  $ showgraph
  @  amend 3
  │
  │ o  r4
  │ │
  │ o  r3
  │ │
  │ o  r2
  │ │
  │ x  r1
  ├─╯
  o  r0
  
  $ hg next --rebase --top
  rebasing 776c07fa2b12 "r2"
  rebasing 137d867d71d5 "r3"
  rebasing daa37004f338 "r4"
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [5d31c6] r4
  $ showgraph
  @  r4
  │
  o  r3
  │
  o  r2
  │
  o  amend 3
  │
  o  r0
  

Rebasing from below the amended changeset with the --newest flag.
  $ reset
  $ hg debugbuilddag -n +6
  $ hg up 'desc(r2)'
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg amend -m "amended" --no-rebase
  hint[amend-restack]: descendants of 776c07fa2b12 are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints
  $ hg up 'desc(r0)'
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ showgraph
  o  amended
  │
  │ o  r5
  │ │
  │ o  r4
  │ │
  │ o  r3
  │ │
  │ x  r2
  ├─╯
  o  r1
  │
  @  r0
  
  $ hg next --rebase --top --newest
  rebasing 137d867d71d5 "r3"
  rebasing daa37004f338 "r4"
  rebasing 5f333e6f7274 "r5"
  5 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [2d8122] r5
  $ showgraph
  @  r5
  │
  o  r4
  │
  o  r3
  │
  o  amended
  │
  o  r1
  │
  o  r0
  

Test aborting due to ambiguity caused by a rebase. The rebase should be
rolled back and the final state should be as it was before `hg next --rebase`.
  $ reset
  $ hg debugbuilddag -n +6
  $ hg up 'desc(r1)'
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg amend -m "amended" --no-rebase
  hint[amend-restack]: descendants of e8ec16b776b6 are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints
  $ mkcommit a
  $ hg prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [f03405] amended
  $ showgraph
  o  add a
  │
  @  amended
  │
  │ o  r5
  │ │
  │ o  r4
  │ │
  │ o  r3
  │ │
  │ o  r2
  │ │
  │ x  r1
  ├─╯
  o  r0
  
  $ hg next --rebase
  rebasing 776c07fa2b12 "r2"
  changeset f03405deb52b has multiple children, namely:
  [c9239a] add a
  [8fb200] r2
  transaction abort!
  rollback completed
  abort: ambiguous next changeset
  (use the --newest or --towards flags to specify which child to pick)
  [255]
  $ showgraph
  o  add a
  │
  @  amended
  │
  │ o  r5
  │ │
  │ o  r4
  │ │
  │ o  r3
  │ │
  │ o  r2
  │ │
  │ x  r1
  ├─╯
  o  r0
  

Test a situation where there is a conflict.
  $ reset
  $ mkcommit a
  $ mkcommit b
  $ mkcommit c
  $ mkcommit d
  $ hg up 7c3bad9141dcb46ff89abf5f61856facd56e476c
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ echo "conflict" > c
  $ hg add c
  $ hg amend -m "amended to add c" --no-rebase
  hint[amend-restack]: descendants of 7c3bad9141dc are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints
  $ showgraph
  @  amended to add c
  │
  │ o  add d
  │ │
  │ o  add c
  │ │
  │ x  add b
  ├─╯
  o  add a
  
  $ hg next --rebase --top
  rebasing 4538525df7e2 "add c"
  merging c
  warning: 1 conflicts while merging c! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ showgraph
  @  amended to add c
  │
  │ o  add d
  │ │
  │ @  add c
  │ │
  │ x  add b
  ├─╯
  o  add a
  
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
  rebasing 4538525df7e2 "add c"
  $ showgraph
  o  add c
  │
  @  amended to add c
  │
  │ o  add d
  │ │
  │ x  add c
  │ │
  │ x  add b
  ├─╯
  o  add a
  
Rebase when other predecessors are still visible
  $ reset
  $ hg debugbuilddag -n +4
  $ hg up 'desc(r1)'
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg amend -m "amended 1" --no-rebase
  hint[amend-restack]: descendants of e8ec16b776b6 are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints
  $ hg next --rebase
  rebasing 776c07fa2b12 "r2"
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [bd2075] r2
  $ hg prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [80573e] amended 1
  $ hg amend -m "amended 2" --no-rebase
  hint[amend-restack]: descendants of 80573e6618ae are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints
  $ showgraph
  @  amended 2
  │
  │ o  r2
  │ │
  │ x  amended 1
  ├─╯
  │ o  r3
  │ │
  │ x  r2
  │ │
  │ x  r1
  ├─╯
  o  r0
  
  $ hg next --rebase
  rebasing bd2075358087 "r2"
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [88a893] r2
  $ showgraph
  @  r2
  │
  o  amended 2
  │
  │ o  r3
  │ │
  │ x  r2
  │ │
  │ x  r1
  ├─╯
  o  r0
  
