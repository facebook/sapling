  $ hg init
  $ echo This is file a1 > a
  $ echo This is file b1 > b
  $ hg add a b
  $ hg commit -m "commit #0"
  $ echo This is file b22 > b
  $ hg commit -m "comment #1"
  $ hg update 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm b
  $ hg commit -A -m "comment #2"
  removing b
  created new head
  $ hg update 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm b
  $ hg update -c 2
  abort: uncommitted changes
  [255]
  $ hg revert b
  $ hg update -c 2
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mv a c

Should abort:

  $ hg update 1
  abort: uncommitted changes
  (commit or update --clean to discard changes)
  [255]
  $ mv c a

Should succeed:

  $ hg update 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
