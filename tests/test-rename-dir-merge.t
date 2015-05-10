  $ hg init t
  $ cd t

  $ mkdir a
  $ echo foo > a/a
  $ echo bar > a/b
  $ hg ci -Am "0"
  adding a/a
  adding a/b

  $ hg co -C 0
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg mv a b
  moving a/a to b/a (glob)
  moving a/b to b/b (glob)
  $ hg ci -m "1 mv a/ b/"

  $ hg co -C 0
  2 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ echo baz > a/c
  $ echo quux > a/d
  $ hg add a/c
  $ hg ci -m "2 add a/c"
  created new head

  $ hg merge --debug 1
    searching for copies back to rev 1
    unmatched files in local:
     a/c
    unmatched files in other:
     b/a
     b/b
    all copies found (* = to merge, ! = divergent, % = renamed and deleted):
     src: 'a/a' -> dst: 'b/a' 
     src: 'a/b' -> dst: 'b/b' 
    checking for directory renames
     discovered dir src: 'a/' -> dst: 'b/'
     pending file src: 'a/c' -> dst: 'b/c'
  resolving manifests
   branchmerge: True, force: False, partial: False
   ancestor: f9b20c0d4c51, local: ce36d17b18fb+, remote: 397f8b00a740
   a/a: other deleted -> r
  removing a/a
   a/b: other deleted -> r
  removing a/b
   b/a: remote created -> g
  getting b/a
   b/b: remote created -> g
  getting b/b
   b/c: remote directory rename - move from a/c -> dm
  moving a/c to b/c (glob)
  3 files updated, 0 files merged, 2 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ echo a/* b/*
  a/d b/a b/b b/c
  $ hg st -C
  M b/a
  M b/b
  A b/c
    a/c
  R a/a
  R a/b
  R a/c
  ? a/d
  $ hg ci -m "3 merge 2+1"
  $ hg debugrename b/c
  b/c renamed from a/c:354ae8da6e890359ef49ade27b68bbc361f3ca88 (glob)

  $ hg co -C 1
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg merge --debug 2
    searching for copies back to rev 1
    unmatched files in local:
     b/a
     b/b
    unmatched files in other:
     a/c
    all copies found (* = to merge, ! = divergent, % = renamed and deleted):
     src: 'a/a' -> dst: 'b/a' 
     src: 'a/b' -> dst: 'b/b' 
    checking for directory renames
     discovered dir src: 'a/' -> dst: 'b/'
     pending file src: 'a/c' -> dst: 'b/c'
  resolving manifests
   branchmerge: True, force: False, partial: False
   ancestor: f9b20c0d4c51, local: 397f8b00a740+, remote: ce36d17b18fb
   b/c: local directory rename - get from a/c -> dg
  getting a/c to b/c
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ echo a/* b/*
  a/d b/a b/b b/c
  $ hg st -C
  A b/c
    a/c
  ? a/d
  $ hg ci -m "4 merge 1+2"
  created new head
  $ hg debugrename b/c
  b/c renamed from a/c:354ae8da6e890359ef49ade27b68bbc361f3ca88 (glob)

Local directory rename with conflicting file added in remote source directory
and untracked in local target directory.

  $ hg co -qC 1
  $ echo target > b/c
  $ hg merge 2
  b/c: untracked file differs
  abort: untracked files in working directory differ from files in requested revision
  [255]
  $ cat b/c
  target
but it should succeed if the content matches
  $ hg cat -r 2 a/c > b/c
  $ hg merge 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg st -C
  A b/c
    a/c
  ? a/d

Local directory rename with conflicting file added in remote source directory
and committed in local target directory.

  $ hg co -qC 1
  $ echo target > b/c
  $ hg add b/c
  $ hg commit -qm 'new file in target directory'
  $ hg merge 2
  merging b/c and a/c to b/c
  warning: conflicts during merge.
  merging b/c incomplete! (edit conflicts, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ hg st -A
  M b/c
    a/c
  ? a/d
  ? b/c.orig
  C b/a
  C b/b
  $ cat b/c
  <<<<<<< local: f1c50ca4f127 - test: new file in target directory
  target
  =======
  baz
  >>>>>>> other: ce36d17b18fb  - test: 2 add a/c
  $ rm b/c.orig

Remote directory rename with conflicting file added in remote target directory
and committed in local source directory.

  $ hg co -qC 2
  $ hg st -A
  ? a/d
  C a/a
  C a/b
  C a/c
  $ hg merge 5
  merging a/c and b/c to b/c
  warning: conflicts during merge.
  merging b/c incomplete! (edit conflicts, then use 'hg resolve --mark')
  2 files updated, 0 files merged, 2 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ hg st -A
  M b/a
  M b/b
  M b/c
    a/c
  R a/a
  R a/b
  R a/c
  ? a/d
  ? b/c.orig
  $ cat b/c
  <<<<<<< local: ce36d17b18fb  - test: 2 add a/c
  baz
  =======
  target
  >>>>>>> other: f1c50ca4f127 - test: new file in target directory

Second scenario with two repos:

  $ cd ..
  $ hg init r1
  $ cd r1
  $ mkdir a
  $ echo foo > a/f
  $ hg add a
  adding a/f (glob)
  $ hg ci -m "a/f == foo"
  $ cd ..

  $ hg clone r1 r2
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd r2
  $ hg mv a b
  moving a/f to b/f (glob)
  $ echo foo1 > b/f
  $ hg ci -m" a -> b, b/f == foo1"
  $ cd ..

  $ cd r1
  $ mkdir a/aa
  $ echo bar > a/aa/g
  $ hg add a/aa
  adding a/aa/g (glob)
  $ hg ci -m "a/aa/g"
  $ hg pull ../r2
  pulling from ../r2
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)

  $ hg merge
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ hg st -C
  M b/f
  A b/aa/g
    a/aa/g
  R a/aa/g
  R a/f

  $ cd ..
