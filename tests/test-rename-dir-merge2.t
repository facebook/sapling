  $ mkdir r1
  $ cd r1
  $ hg init
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
