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
  moving a/a to b/a
  moving a/b to b/b
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
     a/d
    unmatched files in other:
     b/a
     b/b
    all copies found (* = to merge, ! = divergent):
     b/a -> a/a 
     b/b -> a/b 
    checking for directory renames
    dir a/ -> b/
    file a/c -> b/c
    file a/d -> b/d
  resolving manifests
   overwrite None partial False
   ancestor f9b20c0d4c51 local ce36d17b18fb+ remote 397f8b00a740
   a/d: remote renamed directory to b/d -> d
   a/c: remote renamed directory to b/c -> d
   a/b: other deleted -> r
   a/a: other deleted -> r
   b/a: remote created -> g
   b/b: remote created -> g
  updating: a/a 1/6 files (16.67%)
  removing a/a
  updating: a/b 2/6 files (33.33%)
  removing a/b
  updating: a/c 3/6 files (50.00%)
  moving a/c to b/c
  updating: a/d 4/6 files (66.67%)
  moving a/d to b/d
  updating: b/a 5/6 files (83.33%)
  getting b/a
  updating: b/b 6/6 files (100.00%)
  getting b/b
  4 files updated, 0 files merged, 2 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ echo a/* b/*
  a/* b/a b/b b/c b/d
  $ hg st -C
  M b/a
  M b/b
  A b/c
    a/c
  R a/a
  R a/b
  R a/c
  ? b/d
  $ hg ci -m "3 merge 2+1"
  $ hg debugrename b/c
  b/c renamed from a/c:354ae8da6e890359ef49ade27b68bbc361f3ca88

  $ hg co -C 1
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg merge --debug 2
    searching for copies back to rev 1
    unmatched files in local:
     b/a
     b/b
     b/d
    unmatched files in other:
     a/c
    all copies found (* = to merge, ! = divergent):
     b/a -> a/a 
     b/b -> a/b 
    checking for directory renames
    dir a/ -> b/
    file a/c -> b/c
  resolving manifests
   overwrite None partial False
   ancestor f9b20c0d4c51 local 397f8b00a740+ remote ce36d17b18fb
   None: local renamed directory to b/c -> d
  updating:None 1/1 files (100.00%)
  getting a/c to b/c
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ echo a/* b/*
  a/* b/a b/b b/c b/d
  $ hg st -C
  A b/c
    a/c
  ? b/d
  $ hg ci -m "4 merge 1+2"
  created new head
  $ hg debugrename b/c
  b/c renamed from a/c:354ae8da6e890359ef49ade27b68bbc361f3ca88


Second scenario with two repos:

  $ cd ..
  $ hg init r1
  $ cd r1
  $ mkdir a
  $ echo foo > a/f
  $ hg add a
  adding a/f
  $ hg ci -m "a/f == foo"
  $ cd ..

  $ hg clone r1 r2
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd r2
  $ hg mv a b
  moving a/f to b/f
  $ echo foo1 > b/f
  $ hg ci -m" a -> b, b/f == foo1"
  $ cd ..

  $ cd r1
  $ mkdir a/aa
  $ echo bar > a/aa/g
  $ hg add a/aa
  adding a/aa/g
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
