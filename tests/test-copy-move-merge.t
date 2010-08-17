  $ mkdir t
  $ cd t
  $ hg init

  $ echo 1 > a
  $ hg ci -qAm "first" -d "1000000 0"

  $ hg cp a b
  $ hg mv a c
  $ echo 2 >> b
  $ echo 2 >> c

  $ hg ci -qAm "second" -d "1000000 0"

  $ hg co -C 0
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved

  $ echo 0 > a
  $ echo 1 >> a

  $ hg ci -qAm "other" -d "1000000 0"

  $ hg merge --debug
    searching for copies back to rev 1
    unmatched files in other:
     b
     c
    all copies found (* = to merge, ! = divergent):
     c -> a *
     b -> a *
    checking for directory renames
  resolving manifests
   overwrite None partial False
   ancestor 583c7b748052 local fb3948d97f07+ remote 7f1309517659
   a: remote moved to c -> m
   a: remote moved to b -> m
  preserving a for resolve of b
  preserving a for resolve of c
  removing a
  updating: a 1/2 files (50.00%)
  picked tool 'internal:merge' for b (binary False symlink False)
  merging a and b to b
  my b@fb3948d97f07+ other b@7f1309517659 ancestor a@583c7b748052
   premerge successful
  updating: a 2/2 files (100.00%)
  picked tool 'internal:merge' for c (binary False symlink False)
  merging a and c to c
  my c@fb3948d97f07+ other c@7f1309517659 ancestor a@583c7b748052
   premerge successful
  0 files updated, 2 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

file b
  $ cat b
  0
  1
  2

file c
  $ cat c
  0
  1
  2
