  $ . helpers-usechg.sh

Set up test environment.

  $ enable fbamend inhibit rebase
  $ setconfig experimental.evolution.allowdivergence=True
  $ setconfig experimental.evolution="createmarkers, allowunstable"
  $ mkcommit() {
  >   echo "$1" > "$1"
  >   hg add "$1"
  >   hg ci -m "add $1"
  > }
  $ hg init restack && cd restack

Note: Repositories populated by `hg debugbuilddag` don't seem to
correctly show all commits in the log output. Manually creating the
commits results in the expected behavior, so commits are manually
created in the test cases below.

Test unsupported flags:
  $ hg rebase --restack --rev .
  abort: cannot use both --rev and --restack
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
  $ hg rebase --restack --hidden
  abort: cannot use both --hidden and --restack
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
  hint[amend-restack]: descendants of 7c3bad9141dc are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints
  $ showgraph
  @  4 add b
  |
  | o  3 add d
  | |
  | o  2 add c
  | |
  | x  1 add b
  |/
  o  0 add a
  $ hg rebase --restack
  rebasing 2:4538525df7e2 "add c"
  rebasing 3:47d2a3944de8 "add d"
  $ showgraph
  o  6 add d
  |
  o  5 add c
  |
  @  4 add b
  |
  o  0 add a

Test multiple amends of same commit.
  $ newrepo
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
  hint[amend-restack]: descendants of 7c3bad9141dc are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints
  $ echo b >> b
  $ hg amend
  $ showgraph
  @  4 add b
  |
  | o  2 add c
  | |
  | x  1 add b
  |/
  o  0 add a
  $ hg rebase --restack
  rebasing 2:4538525df7e2 "add c"
  $ showgraph
  o  5 add c
  |
  @  4 add b
  |
  o  0 add a

Test conflict during rebasing.
  $ newrepo
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
  hint[amend-restack]: descendants of 7c3bad9141dc are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints
  $ showgraph
  @  5 add b
  |
  | o  4 add e
  | |
  | o  3 add d
  | |
  | o  2 add c
  | |
  | x  1 add b
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
  o  8 add e
  |
  o  7 add d
  |
  o  6 add c
  |
  @  5 add b
  |
  o  0 add a

Test finding a stable base commit from within the old stack.
  $ newrepo
  $ mkcommit a
  $ mkcommit b
  $ mkcommit c
  $ mkcommit d
  $ hg up 1
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ echo b >> b
  $ hg amend
  hint[amend-restack]: descendants of 7c3bad9141dc are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints
  $ hg up 3
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ showgraph
  o  4 add b
  |
  | @  3 add d
  | |
  | o  2 add c
  | |
  | x  1 add b
  |/
  o  0 add a
  $ hg rebase --restack
  rebasing 2:4538525df7e2 "add c"
  rebasing 3:47d2a3944de8 "add d"
  $ showgraph
  @  6 add d
  |
  o  5 add c
  |
  o  4 add b
  |
  o  0 add a

Test finding a stable base commit from a new child of the amended commit.
  $ newrepo
  $ mkcommit a
  $ mkcommit b
  $ mkcommit c
  $ mkcommit d
  $ hg up 1
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ echo b >> b
  $ hg amend
  hint[amend-restack]: descendants of 7c3bad9141dc are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints
  $ mkcommit e
  $ showgraph
  @  5 add e
  |
  o  4 add b
  |
  | o  3 add d
  | |
  | o  2 add c
  | |
  | x  1 add b
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
  | @  5 add e
  |/
  o  4 add b
  |
  o  0 add a

Test finding a stable base commit when there are multiple amends and
a commit on top of one of the obsolete intermediate commits.
  $ newrepo
  $ mkcommit a
  $ mkcommit b
  $ mkcommit c
  $ mkcommit d
  $ hg up 1
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ echo b >> b
  $ hg amend
  hint[amend-restack]: descendants of 7c3bad9141dc are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints
  $ mkcommit e
  $ hg prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [*] add b (glob)
  $ echo b >> b
  $ hg amend
  hint[amend-restack]: descendants of c54ee8acf83d are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints
  $ hg up 5
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ showgraph
  o  6 add b
  |
  | @  5 add e
  | |
  | x  4 add b
  |/
  | o  3 add d
  | |
  | o  2 add c
  | |
  | x  1 add b
  |/
  o  0 add a
  $ hg rebase --restack
  rebasing 2:4538525df7e2 "add c"
  rebasing 3:47d2a3944de8 "add d"
  rebasing 5:c1992d8998fa "add e"
  $ showgraph
  @  9 add e
  |
  | o  8 add d
  | |
  | o  7 add c
  |/
  o  6 add b
  |
  o  0 add a

Test that we start from the bottom of the stack. (Previously, restack would
only repair the unstable children closest to the current changeset. This
behavior is now incorrect -- restack should always fix the whole stack.)
  $ newrepo
  $ mkcommit a
  $ mkcommit b
  $ mkcommit c
  $ mkcommit d
  $ hg up 1
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ echo b >> b
  $ hg amend
  hint[amend-restack]: descendants of 7c3bad9141dc are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints
  $ hg up 2
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo c >> c
  $ hg amend
  hint[amend-restack]: descendants of 4538525df7e2 are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints
  $ hg up 3
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ showgraph
  o  5 add c
  |
  | o  4 add b
  | |
  | | @  3 add d
  | | |
  +---x  2 add c
  | |
  x |  1 add b
  |/
  o  0 add a
  $ hg rebase --restack
  rebasing 5:a43fcd08f41f "add c" (tip)
  rebasing 3:47d2a3944de8 "add d"
  $ showgraph
  @  7 add d
  |
  o  6 add c
  |
  o  4 add b
  |
  o  0 add a

Test what happens if there is no base commit found. The command should
fix up everything above the current commit, leaving other commits
below the current commit alone.
  $ newrepo
  $ mkcommit a
  $ mkcommit b
  $ mkcommit c
  $ mkcommit d
  $ mkcommit e
  $ hg up 3
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo d >> d
  $ hg amend
  hint[amend-restack]: descendants of 47d2a3944de8 are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints
  $ hg up 0
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved
  $ mkcommit f
  $ hg up 1
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ showgraph
  o  6 add f
  |
  | o  5 add d
  | |
  | | o  4 add e
  | | |
  | | x  3 add d
  | |/
  | o  2 add c
  | |
  | @  1 add b
  |/
  o  0 add a
  $ hg rebase --restack
  rebasing 4:9d206ffc875e "add e"
  $ showgraph
  o  7 add e
  |
  | o  6 add f
  | |
  o |  5 add d
  | |
  o |  2 add c
  | |
  @ |  1 add b
  |/
  o  0 add a

Test having an unamended commit.
  $ newrepo
  $ mkcommit a
  $ mkcommit b
  $ mkcommit c
  $ hg prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [*] add b (glob)
  $ echo b >> b
  $ hg amend -m "Amended"
  hint[amend-restack]: descendants of 7c3bad9141dc are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints
  $ echo b >> b
  $ hg amend -m "Unamended"
  $ hg unamend
  $ hg up -C 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ showgraph
  o  3 Amended
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
  o  5 add c
  |
  @  3 Amended
  |
  | x  1 add b
  |/
  o  0 add a

Revision 2 "add c" is already stable (not orphaned) so restack does nothing:

  $ hg rebase --restack
  nothing to rebase - empty destination

Test recursive restacking -- basic case.
  $ newrepo
  $ mkcommit a
  $ mkcommit b
  $ mkcommit c
  $ mkcommit d
  $ hg up 1
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ echo b >> b
  $ hg amend
  hint[amend-restack]: descendants of 7c3bad9141dc are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints
  $ hg up 2
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo c >> c
  $ hg amend
  hint[amend-restack]: descendants of 4538525df7e2 are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints
  $ hg up 1
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ showgraph
  o  5 add c
  |
  | o  4 add b
  | |
  | | o  3 add d
  | | |
  +---x  2 add c
  | |
  @ |  1 add b
  |/
  o  0 add a
  $ hg rebase --restack
  rebasing 5:a43fcd08f41f "add c" (tip)
  rebasing 3:47d2a3944de8 "add d"
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ showgraph
  o  7 add d
  |
  o  6 add c
  |
  @  4 add b
  |
  | x  1 add b
  |/
  o  0 add a

Test recursive restacking -- more complex case. This test is designed to
to check for a bug encountered if rebasing is performed naively from the
bottom-up wherein obsolescence information for commits further up the
stack is lost upon rebasing lower levels.
  $ newrepo
  $ mkcommit a
  $ mkcommit b
  $ mkcommit c
  $ mkcommit d
  $ hg up 1
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ echo b >> b
  $ hg amend
  hint[amend-restack]: descendants of 7c3bad9141dc are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints
  $ mkcommit e
  $ mkcommit f
  $ hg prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [*] add e (glob)
  $ echo e >> e
  $ hg amend
  hint[amend-restack]: descendants of c1992d8998fa are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints
  $ hg up 2
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo c >> c
  $ hg amend
  hint[amend-restack]: descendants of 4538525df7e2 are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints
  $ mkcommit g
  $ mkcommit h
  $ hg prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [*] add g (glob)
  $ echo g >> g
  $ hg amend
  hint[amend-restack]: descendants of 0261378a5dc1 are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints
  $ hg up 1
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ showgraph
  o  11 add g
  |
  | o  10 add h
  | |
  | x  9 add g
  |/
  o  8 add c
  |
  | o  7 add e
  | |
  | | o  6 add f
  | | |
  | | x  5 add e
  | |/
  | o  4 add b
  | |
  | | o  3 add d
  | | |
  +---x  2 add c
  | |
  @ |  1 add b
  |/
  o  0 add a
  $ hg rebase --restack
  rebasing 6:2a79e3a98cd6 "add f"
  rebasing 8:a43fcd08f41f "add c"
  rebasing 11:604f34a1983d "add g" (tip)
  rebasing 3:47d2a3944de8 "add d"
  rebasing 10:9f2a7cefd4b4 "add h"
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ showgraph
  o  16 add h
  |
  | o  15 add d
  | |
  o |  14 add g
  |/
  o  13 add c
  |
  | o  12 add f
  | |
  | o  7 add e
  |/
  @  4 add b
  |
  | x  1 add b
  |/
  o  0 add a
