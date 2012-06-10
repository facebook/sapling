  $ hg init base

  $ cd base
  $ echo 'alpha' > alpha
  $ hg ci -A -m 'add alpha'
  adding alpha
  $ cd ..

  $ hg clone base work
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cd work
  $ echo 'beta' > beta
  $ hg ci -A -m 'add beta'
  adding beta
  $ cd ..

  $ cd base
  $ echo 'gamma' > gamma
  $ hg ci -A -m 'add gamma'
  adding gamma
  $ cd ..

  $ cd work
  $ hg pull -q
  $ hg merge
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

Update --clean to revision 1 to simulate a failed merge:

  $ rm alpha beta gamma
  $ hg update --clean 1
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cd ..
