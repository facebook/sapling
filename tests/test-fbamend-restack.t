Set up test environment.
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > drawdag=$RUNTESTDIR/drawdag.py
  > fbamend=$TESTDIR/../hgext3rd/fbamend
  > inhibit=$TESTDIR/../hgext3rd/inhibit.py
  > rebase=
  > [experimental]
  > evolution.allowdivergence = True
  > evolution = createmarkers, allowunstable
  > [fbamend]
  > # do not write preamend bookmarks
  > userestack = True
  > EOF
  $ mkcommit() {
  >   echo "$1" > "$1"
  >   hg add "$1"
  >   hg ci -m "add $1"
  > }
  $ reset() {
  >   cd ..
  >   rm -rf restack
  >   hg init restack
  >   cd restack
  > }
  $ showgraph() {
  >   hg log --graph -T "{rev} {desc|firstline}" | sed \$d
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
  warning: the changeset's children were left behind
  (use 'hg restack' to rebase them)
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
  (use 'hg restack' to rebase them)
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
  (use 'hg restack' to rebase them)
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
  (use 'hg restack' to rebase them)
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
  (use 'hg restack' to rebase them)
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
  (use 'hg restack' to rebase them)
  $ mkcommit e
  $ hg prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [*] add b (glob)
  $ echo b >> b
  $ hg amend
  warning: the changeset's children were left behind
  (use 'hg restack' to rebase them)
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
  (use 'hg restack' to rebase them)
  $ hg up 2
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo c >> c
  $ hg amend
  warning: the changeset's children were left behind
  (use 'hg restack' to rebase them)
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
  (use 'hg restack' to rebase them)
  $ hg up 0
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved
  $ mkcommit f
  created new head
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
  (use 'hg restack' to rebase them)
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

Test situation with divergence. Restack should rebase unstable children
onto the newest successor of their parent.
  $ reset
  $ mkcommit a
  $ mkcommit b
  $ mkcommit c
  $ hg prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [*] add b (glob)
  $ hg amend -m "successor 1"
  warning: the changeset's children were left behind
  (use 'hg restack' to rebase them)
  $ hg up 1
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg amend -m "successor 2"
  warning: the changeset's children were left behind
  (use 'hg restack' to rebase them)
  $ hg up 1
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ showgraph
  o  4 successor 2
  |
  | o  3 successor 1
  |/
  | o  2 add c
  | |
  | @  1 add b
  |/
  o  0 add a
  $ hg rebase --restack
  rebasing 2:4538525df7e2 "add c"
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ showgraph
  o  5 add c
  |
  @  4 successor 2
  |
  | o  3 successor 1
  |/
  | x  1 add b
  |/
  o  0 add a

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
  (use 'hg restack' to rebase them)
  $ showgraph
  @  3 add b
  |
  | o  2 add c
  | |
  | x  1 add b
  |/
  o  0 add a
  $ hg up 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo c >> b
  $ hg amend
  warning: the changeset's children were left behind
  (use 'hg restack' to rebase them)
  $ showgraph
  @  4 add b
  |
  | o  3 add b
  |/
  | o  2 add c
  | |
  | x  1 add b
  |/
  o  0 add a
  $ hg unamend
  $ hg up -C 3
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ showgraph
  @  3 add b
  |
  | o  2 add c
  | |
  | o  1 add b
  |/
  o  0 add a

Revision 2 "add c" is already stable (not orphaned) so restack does nothing:

  $ hg rebase --restack
  nothing to rebase - empty destination

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
  (use 'hg restack' to rebase them)
  $ hg up 2
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo c >> c
  $ hg amend
  warning: the changeset's children were left behind
  (use 'hg restack' to rebase them)
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
  (use 'hg restack' to rebase them)
  $ mkcommit e
  $ mkcommit f
  $ hg prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [*] add e (glob)
  $ echo e >> e
  $ hg amend
  warning: the changeset's children were left behind
  (use 'hg restack' to rebase them)
  $ hg up 2
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo c >> c
  $ hg amend
  warning: the changeset's children were left behind
  (use 'hg restack' to rebase them)
  $ mkcommit g
  $ mkcommit h
  $ hg prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [*] add g (glob)
  $ echo g >> g
  $ hg amend
  warning: the changeset's children were left behind
  (use 'hg restack' to rebase them)
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

Restack does topological sort and only rebases "D" once:

  $ reset
  $ hg debugdrawdag<<'EOS'
  > D
  > |
  > C
  > |
  > B
  > |
  > A
  > EOS
  $ hg update B -q
  $ hg commit --amend -m B2 -q 2>/dev/null
  $ hg tag --local B2
  $ hg rebase -r C -d B2 -q
  $ hg commit --amend -m B3 -q 2>/dev/null
  $ hg tag --local B3
  $ showgraph
  @  6 B3
  |
  | o  5 C
  | |
  | x  4 B2
  |/
  | o  3 D
  | |
  | x  2 C
  | |
  | x  1 B
  |/
  o  0 A
  $ hg rebase --restack
  rebasing 5:ca53c8ceb284 "C"
  rebasing 3:f585351a92f8 "D" (D)
  $ showgraph
  o  8 D
  |
  o  7 C
  |
  @  6 B3
  |
  | x  4 B2
  |/
  | x  3 D
  | |
  | x  2 C
  | |
  | x  1 B
  |/
  o  0 A

Restack will only restack the "current" stack and leave other stacks untouched.

  $ reset
  $ hg debugdrawdag<<'EOS'
  >  D   H   K
  >  |   |   |
  >  B C F G J L    # amend: B -> C
  >  |/  |/  |/     # amend: F -> G
  >  A   E   I   Z  # amend: J -> L
  > EOS

  $ hg phase --public -r Z+I+A+E

  $ hg update -q Z
  $ hg rebase --restack
  nothing to restack
  [1]

  $ hg update -q D
  $ hg rebase --restack
  rebasing 10:be0ef73c17ad "D" (D)

  $ hg update -q G
  $ hg rebase --restack
  rebasing 11:cc209258a732 "H" (H)

  $ hg update -q I
  $ hg rebase --restack
  rebasing 12:59760668f0e1 "K" (K)

  $ rm .hg/localtags
  $ showgraph
  o  15 K
  |
  | o  14 H
  | |
  | | o  13 D
  | | |
  o | |  9 L
  | | |
  | o |  7 G
  | | |
  | | o  5 C
  | | |
  | | | o  3 Z
  | | |
  @ | |  2 I
   / /
  o /  1 E
   /
  o  0 A

The "prune" cases.

  $ reset
  $ hg debugdrawdag<<'EOS'
  > D E
  > |/
  > C
  > |       # amend: F -> F2
  > B  G H  # prune: A, C, F2
  > |  |/
  > A  F F2
  > EOS

  $ hg update -q B
  $ hg rebase --restack
  rebasing 3:112478962961 "B" (B)
  rebasing 7:f585351a92f8 "D" (D)
  rebasing 8:78d2dca436b2 "E" (E tip)

  $ hg update -q H
  $ hg rebase --restack
  rebasing 4:8fdb2c1feb20 "G" (G)
  rebasing 5:02ac06fe83b9 "H" (H)

  $ rm .hg/localtags
  $ showgraph
  @  13 H
  
  o  12 G
  
  o  11 E
  |
  | o  10 D
  |/
  o  9 B

Restack could resume after resolving merge conflicts.

  $ reset
  $ hg debugdrawdag<<'EOS'
  >  F   G    # F/C = F # cause conflict
  >  |   |    # G/E = G # cause conflict
  >  B C D E  # amend: B -> C
  >  |/  |/   # amend: D -> E
  >  |   /
  >  |  /
  >  | /
  >  |/
  >  A
  > EOS

  $ hg update -q F
  $ hg rebase --restack
  rebasing 5:ed8545a5c22a "F" (F)
  merging C
  warning: conflicts while merging C! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

  $ rm .hg/localtags

  $ echo R > C
  $ hg resolve --mark -q
  continue: hg rebase --continue
  $ hg rebase --continue
  rebasing 5:ed8545a5c22a "F"
  rebasing 6:4d1ef7d890c5 "G" (tip)
  merging E
  warning: conflicts while merging E! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

  $ echo R > E
  $ hg resolve --mark -q
  continue: hg rebase --continue
  $ hg rebase --continue
  already rebased 5:ed8545a5c22a "F" as 2282fe522d5c
  rebasing 6:4d1ef7d890c5 "G"

  $ showgraph
  o  8 G
  |
  | @  7 F
  | |
  o |  4 E
  | |
  | o  2 C
  |/
  o  0 A
