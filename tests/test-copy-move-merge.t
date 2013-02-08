  $ hg init t
  $ cd t

  $ echo 1 > a
  $ hg ci -qAm "first"

  $ hg cp a b
  $ hg mv a c
  $ echo 2 >> b
  $ echo 2 >> c

  $ hg ci -qAm "second"

  $ hg co -C 0
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved

  $ echo 0 > a
  $ echo 1 >> a

  $ hg ci -qAm "other"

  $ hg merge --debug
    searching for copies back to rev 1
    unmatched files in other:
     b
     c
    all copies found (* = to merge, ! = divergent, % = renamed and deleted):
     src: 'a' -> dst: 'b' *
     src: 'a' -> dst: 'c' *
    checking for directory renames
  resolving manifests
   branchmerge: True, force: False, partial: False
   ancestor: b8bf91eeebbc, local: add3f11052fa+, remote: 17c05bb7fcb6
   a: remote moved to b -> m
    preserving a for resolve of b
   a: remote moved to c -> m
    preserving a for resolve of c
  removing a
  updating: a 1/2 files (50.00%)
  picked tool 'internal:merge' for b (binary False symlink False)
  merging a and b to b
  my b@add3f11052fa+ other b@17c05bb7fcb6 ancestor a@b8bf91eeebbc
   premerge successful
  updating: a 2/2 files (100.00%)
  picked tool 'internal:merge' for c (binary False symlink False)
  merging a and c to c
  my c@add3f11052fa+ other c@17c05bb7fcb6 ancestor a@b8bf91eeebbc
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

  $ cd ..
