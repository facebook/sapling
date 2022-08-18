#chg-compatible
#debugruntest-compatible
#inprocess-hg-incompatible

UTILS:
  $ reset() {
  >   cd ..
  >   rm -rf a
  >   hg init a
  >   cd a
  > }

TEST: incomplete requirements handling (required extension excluded)
  $ hg init a
  $ cd a
  $ enable drop

  $ hg drop 1
  extension rebase not found
  abort: required extensions not detected
  [255]

SETUP: Properly setup all required extensions
  $ enable rebase drop

TEST: handling no revision provided to drop
  $ hg drop
  abort: no revision to drop was provided
  [255]

TEST: aborting when drop called on root changeset
  $ hg debugbuilddag +1
  $ hg log -G -T '{desc|firstline}'
  o  r0
  
  $ hg drop -r 'desc(r0)'
  abort: root changeset cannot be dropped
  [255]

  $ hg log -G -T '{desc|firstline}'
  o  r0
  
RESET and SETUP
  $ reset
  $ enable rebase drop

TEST: dropping changeset in the middle of the stack
  $ hg debugbuilddag +4 -m
  $ hg log -G -T '{desc|firstline}'
  o  r3
  │
  o  r2
  │
  o  r1
  │
  o  r0
  
  $ hg drop -r 'desc(r2)'
  Dropping changeset c175ba: r2
  rebasing c034855f2b01 "r3"
  merging mf
  $ hg log -G -T '{desc|firstline}'
  o  r3
  │
  o  r1
  │
  o  r0
  
TEST: abort when more than one revision provided
  $ hg drop -r 1 4
  abort: only one revision can be dropped at a time
  [255]

RESET and SETUP
  $ reset
  $ enable rebase drop

TEST: dropping a changest with child changesets
  $ hg debugbuilddag -m "+5 *3 +2"
  $ hg log -G -T '{desc|firstline}'
  o  r7
  │
  o  r6
  │
  o  r5
  │
  │ o  r4
  │ │
  │ o  r3
  ├─╯
  o  r2
  │
  o  r1
  │
  o  r0
  
  $ hg drop 'desc(r2)'
  Dropping changeset 37d4c1: r2
  rebasing a422badec216 "r3"
  merging mf
  rebasing b762560d23fd "r4"
  merging mf
  rebasing e76b6544a13a "r5"
  merging mf
  rebasing 4905937520ff "r6"
  merging mf
  rebasing 2c7cfba83429 "r7"
  merging mf
  $ hg log -G -T '{desc|firstline}'
  o  r7
  │
  o  r6
  │
  o  r5
  │
  │ o  r4
  │ │
  │ o  r3
  ├─╯
  o  r1
  │
  o  r0
  
TEST: aborting drop on merge changeset

  $ hg checkout 'max(desc(r3))'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg merge 'max(desc(r7))'
  merging mf
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg commit -m "merge"
  $ hg log -G -T '{desc|firstline}'
  @    merge
  ├─╮
  │ o  r7
  │ │
  │ o  r6
  │ │
  │ o  r5
  │ │
  │ │ o  r4
  ├───╯
  o │  r3
  ├─╯
  o  r1
  │
  o  r0
  
  $ hg drop 'desc(merge)'
  abort: merge changeset cannot be dropped
  [255]

TEST: abort when dropping a public changeset
  $ hg debugmakepublic -r 'desc(r1)'
  $ hg drop 'desc(r1)'
  abort: public changeset which landed cannot be dropped
  [255]

RESET and SETUP
  $ reset
  $ enable rebase drop

TEST: dropping a changeset with merge conflict
  $ hg debugbuilddag -o +4
  $ hg log -G -T '{desc|firstline}'
  o  r3
  │
  o  r2
  │
  o  r1
  │
  o  r0
  
  $ hg drop 'desc(r1)'
  Dropping changeset 2a8ed6: r1
  rebasing 3d69e4d36b46 "r2"
  merging of
  warning: 1 conflicts while merging of! (edit, then use 'hg resolve --mark')
  conflict occurred during drop: please fix it by running 'hg rebase --continue', and then re-run 'hg drop'
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
