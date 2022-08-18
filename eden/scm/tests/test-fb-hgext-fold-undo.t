#chg-compatible
#debugruntest-compatible
#inprocess-hg-incompatible

Set up test environment.
This test confirms cacheinvalidation in hg fold.
  $ configure mutation-norecord
  $ enable amend rebase undo
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
  o  9c9414e0356c r7
  │
  o  ec6d8e65acbe r6
  │
  o  77d787dfa5b6 r5
  │
  │ o  b762560d23fd r4
  │ │
  │ o  a422badec216 r3
  │ │
  │ o  37d4c1cec295 r2
  ├─╯
  o  f177fbb9e8d1 r1
  │
  o  93cbaf5e6529 r0
  $ hg up 'desc(r7)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

Do a fold
  $ hg fold --exact 7 6
  2 changesets folded
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ showgraph
  @  ecdfa824af18 r7
  │
  o  77d787dfa5b6 r5
  │
  │ o  b762560d23fd r4
  │ │
  │ o  a422badec216 r3
  │ │
  │ o  37d4c1cec295 r2
  ├─╯
  o  f177fbb9e8d1 r1
  │
  o  93cbaf5e6529 r0

Do an undo
  $ hg undo
  undone to *, before fold --exact 7 6 (glob)
  $ showgraph
  @  9c9414e0356c r7
  │
  o  ec6d8e65acbe r6
  │
  o  77d787dfa5b6 r5
  │
  │ o  b762560d23fd r4
  │ │
  │ o  a422badec216 r3
  │ │
  │ o  37d4c1cec295 r2
  ├─╯
  o  f177fbb9e8d1 r1
  │
  o  93cbaf5e6529 r0
