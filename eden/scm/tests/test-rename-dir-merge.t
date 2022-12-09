#chg-compatible
#debugruntest-compatible
#inprocess-hg-incompatible
  $ setconfig experimental.allowfilepeer=True

  $ hg init t
  $ cd t

  $ mkdir a
  $ echo foo > a/a
  $ echo bar > a/b
  $ hg ci -Am "0"
  adding a/a
  adding a/b

  $ hg co -C 'desc(0)'
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg mv a b
  moving a/a to b/a
  moving a/b to b/b
  $ hg ci -m "1 mv a/ b/"

  $ hg co -C 'desc(0)'
  2 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ echo baz > a/c
  $ echo quux > a/d
  $ hg add a/c
  $ hg ci -m "2 add a/c"

  $ hg merge --debug 'desc(1)'
    searching for copies back to * (glob)
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
   ancestor: *, local: *+, remote: * (glob)
   a/a: other deleted -> r
  removing a/a
   a/b: other deleted -> r
  removing a/b
   b/a: remote created -> g
  getting b/a
   b/b: remote created -> g
  getting b/b
   b/c: remote directory rename - move from a/c -> dm
  moving a/c to b/c
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
  b/c renamed from a/c:354ae8da6e890359ef49ade27b68bbc361f3ca88

  $ hg co -C 'desc("1 mv a/ b/")'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg merge --debug 'desc("2 add a/c")'
    searching for copies back to * (glob)
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
   ancestor: *, local: *+, remote: * (glob)
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
  $ hg debugrename b/c
  b/c renamed from a/c:354ae8da6e890359ef49ade27b68bbc361f3ca88

Local directory rename with conflicting file added in remote source directory
and untracked in local target directory.

  $ hg co -qC 'desc("1 mv a/ b/")'
  $ echo target > b/c
  $ hg merge 'desc("2 add a/c")'
  b/c: untracked file differs
  abort: untracked files in working directory differ from files in requested revision
  [255]
  $ cat b/c
  target
but it should succeed if the content matches
  $ hg cat -r 'desc("2 add a/c")' a/c > b/c
  $ hg merge 'desc("2 add a/c")'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg st -C
  A b/c
    a/c
  ? a/d

Local directory rename with conflicting file added in remote source directory
and committed in local target directory.

  $ hg co -qC 'desc("1 mv a/ b/")'
  $ echo target > b/c
  $ hg add b/c
  $ hg commit -qm 'new file in target directory'
  $ hg merge 'desc("2 add a/c")'
  merging b/c and a/c to b/c
  warning: 1 conflicts while merging b/c! (edit, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]
  $ hg st -A
  M b/c
    a/c
  ? a/d
  ? b/c.orig
  C b/a
  C b/b
  $ cat b/c
  <<<<<<< working copy: * - test: new file in target directory (glob)
  target
  =======
  baz
  >>>>>>> merge rev:    * - test: 2 add a/c (glob)
  $ rm b/c.orig

Remote directory rename with conflicting file added in remote target directory
and committed in local source directory.

  $ hg co -qC 'desc("2 add a/c")'
  $ hg st -A
  ? a/d
  C a/a
  C a/b
  C a/c
  $ hg merge 'desc(new)'
  merging a/c and b/c to b/c
  warning: 1 conflicts while merging b/c! (edit, then use 'hg resolve --mark')
  2 files updated, 0 files merged, 2 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
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
  <<<<<<< working copy: * - test: 2 add a/c (glob)
  baz
  =======
  target
  >>>>>>> merge rev:    * - test: new file in target directory (glob)

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

Test renames to separate directories

  $ hg init a
  $ cd a
  $ mkdir a
  $ touch a/s
  $ touch a/t
  $ hg ci -Am0
  adding a/s
  adding a/t

Add more files

  $ touch a/s2
  $ touch a/t2
  $ hg ci -Am1
  adding a/s2
  adding a/t2

Do moves on a branch

  $ hg up 'desc(0)'
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ mkdir s
  $ mkdir t
  $ hg mv a/s s
  $ hg mv a/t t
  $ hg ci -Am2
  $ hg st --copies --change .
  A s/s
    a/s
  A t/t
    a/t
  R a/s
  R a/t

Merge shouldn't move s2, t2

  $ hg merge
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg st --copies
  M a/s2
  M a/t2

Try the merge in the other direction. It may or may not be appropriate for
status to list copies here.

  $ hg up -C 'desc(1)'
  4 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg merge
  2 files updated, 0 files merged, 2 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg st --copies
  M s/s
  M t/t
  R a/s
  R a/t
