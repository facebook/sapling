Test update logic when there are renames or weird same-name cases between dirs
and files

Update with local changes across a file rename

  $ hg init r1 && cd r1

  $ echo a > a
  $ hg add a
  $ hg ci -m a

  $ hg mv a b
  $ hg ci -m rename

  $ echo b > b
  $ hg ci -m change

  $ hg up -q 0

  $ echo c > a

  $ hg up
  merging a and b to b
  warning: conflicts while merging b! (edit, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges
  [1]

Test update when local untracked directory exists with the same name as a
tracked file in a commit we are updating to
  $ hg init r2 && cd r2
  $ echo root > root && hg ci -Am root  # rev 0
  adding root
  $ echo text > name && hg ci -Am "name is a file"  # rev 1
  adding name
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkdir name
  $ hg up 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

Test update when local untracked directory exists with some files in it and has
the same name a tracked file in a commit we are updating to. In future this
should be updated to give an friendlier error message, but now we should just
make sure that this does not erase untracked data
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkdir name
  $ echo text > name/file
  $ hg st
  ? name/file
  $ hg up 1
  abort: *: '$TESTTMP/r1/r2/name' (glob)
  [255]
  $ cd ..
