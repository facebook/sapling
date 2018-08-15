Set up test environment.
This test confirms cacheinvalidation in hg fold.
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > fbamend=
  > inhibit=
  > undo=
  > rebase=
  > [experimental]
  > evolution = createmarkers, allowunstable
  > EOF
  $ reset() {
  >   cd ..
  >   rm -rf repo
  >   hg init repo
  >   cd repo
  > }

Set up repo.
  $ hg init repo && cd repo
  $ hg debugbuilddag -m "+5 *4 +2"
  $ showgraph
  o  7 r7
  |
  o  6 r6
  |
  o  5 r5
  |
  | o  4 r4
  | |
  | o  3 r3
  | |
  | o  2 r2
  |/
  o  1 r1
  |
  o  0 r0
  $ hg up 7
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

Do a fold
  $ hg fold --exact 7 6
  2 changesets folded
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ showgraph
  @  8 r7
  |
  o  5 r5
  |
  | o  4 r4
  | |
  | o  3 r3
  | |
  | o  2 r2
  |/
  o  1 r1
  |
  o  0 r0

Do an undo
  $ hg undo
  undone to *, before fold --exact 7 6 (glob)
  $ showgraph
  @  7 r7
  |
  o  6 r6
  |
  o  5 r5
  |
  | o  4 r4
  | |
  | o  3 r3
  | |
  | o  2 r2
  |/
  o  1 r1
  |
  o  0 r0
