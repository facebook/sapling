test that we don't interrupt the merge session if
a file-level merge failed

  $ hg init repo
  $ cd repo

  $ echo foo > foo
  $ echo a > bar
  $ hg ci -Am 'add foo'
  adding bar
  adding foo

  $ hg mv foo baz
  $ echo b >> bar
  $ echo quux > quux1
  $ hg ci -Am 'mv foo baz'
  adding quux1

  $ hg up -qC 0
  $ echo >> foo
  $ echo c >> bar
  $ echo quux > quux2
  $ hg ci -Am 'change foo'
  adding quux2
  created new head

test with the rename on the remote side
  $ HGMERGE=false hg merge
  merging bar
  merging bar failed!
  merging foo and baz to baz
  1 files updated, 1 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ hg resolve -l
  U bar
  R baz

test with the rename on the local side
  $ hg up -C 1
  3 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ HGMERGE=false hg merge
  merging bar
  merging bar failed!
  merging baz and foo to baz
  1 files updated, 1 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]

show unresolved
  $ hg resolve -l
  U bar
  R baz

unmark baz
  $ hg resolve -u baz

show
  $ hg resolve -l
  U bar
  U baz
  $ hg st
  M bar
  M baz
  M quux2
  ? bar.orig

re-resolve baz
  $ hg resolve baz
  merging baz and foo to baz

after resolve
  $ hg resolve -l
  U bar
  R baz

resolve all warning
  $ hg resolve
  abort: no files or directories specified; use --all to remerge all files
  [255]

resolve all
  $ hg resolve -a
  merging bar
  warning: conflicts during merge.
  merging bar incomplete! (edit conflicts, then use 'hg resolve --mark')
  [1]

after
  $ hg resolve -l
  U bar
  R baz

  $ cd ..
