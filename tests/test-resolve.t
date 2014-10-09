test that a commit clears the merge state.

  $ hg init repo
  $ cd repo

  $ echo foo > file1
  $ echo foo > file2
  $ hg commit -Am 'add files'
  adding file1
  adding file2

  $ echo bar >> file1
  $ echo bar >> file2
  $ hg commit -Am 'append bar to files'

create a second head

  $ hg up -C 0
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo baz >> file1
  $ echo baz >> file2
  $ hg commit -Am 'append baz to files'
  created new head

failing merge

  $ hg merge --tool=internal:fail
  0 files updated, 0 files merged, 0 files removed, 2 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]

resolve -l should contain unresolved entries

  $ hg resolve -l
  U file1
  U file2

resolving an unknown path should emit a warning

  $ hg resolve -m does-not-exist
  arguments do not match paths that need resolving

resolve the failure

  $ echo resolved > file1
  $ hg resolve -m file1

resolve -l should show resolved file as resolved

  $ hg resolve -l
  R file1
  U file2

resolve -m without paths should mark all resolved

  $ hg resolve -m
  (no more unresolved files)
  $ hg commit -m 'resolved'

resolve -l should be empty after commit

  $ hg resolve -l

resolve -m should abort when no merge in progress

  $ hg resolve -m
  abort: resolve command not applicable when not merging
  [255]

test crashed merge with empty mergestate

  $ mkdir .hg/merge
  $ touch .hg/merge/state

resolve -l should be empty

  $ hg resolve -l

  $ cd ..
