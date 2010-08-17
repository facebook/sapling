  $ mkdir t
  $ cd t
  $ hg init
  $ echo This is file a1 > a
  $ echo This is file b1 > b
  $ hg add a b
  $ hg commit -m "commit #0" -d "1000000 0"
  $ echo This is file b22 > b
  $ hg commit -m"comment #1" -d "1000000 0"
  $ hg update 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm b
  $ hg commit -A -m"comment #2" -d "1000000 0"
  removing b
  created new head
  $ mv a c
in theory, we shouldn't need the "-y" below, but it prevents
this test from hanging when "hg update" erroneously prompts the
user for "keep or delete"

should abort
  $ hg update -y 1
  abort: crosses branches (use 'hg merge' to merge or use 'hg update -C' to discard changes)
  $ mv c a
should succeed
  $ hg update -y 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ exit 0
