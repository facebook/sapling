test that a commit clears the merge state.

  $ hg init repo
  $ cd repo

  $ echo foo > file
  $ hg commit -Am 'add file'
  adding file

  $ echo bar >> file
  $ hg commit -Am 'append bar'


create a second head

  $ hg up -C 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo baz >> file
  $ hg commit -Am 'append baz'
  created new head

failing merge

  $ hg merge --tool=internal:fail
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]

resolve -l should contain an unresolved entry

  $ hg resolve -l
  U file

resolving an unknown path emits a warning
  $ hg resolve -m does-not-exist
  arguments do not match paths that need resolved

resolve the failure

  $ echo resolved > file
  $ hg resolve -m file
  no unresolved files; you may continue your unfinished operation
  $ hg commit -m 'resolved'

resolve -l should error since no merge in progress

  $ hg resolve -l
  abort: resolve command not applicable when not merging
  [255]

test crashed merge with empty mergestate

  $ mkdir .hg/merge
  $ touch .hg/merge/state

resolve -l, should be empty

  $ hg resolve -l

  $ cd ..
