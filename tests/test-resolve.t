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

create a second head with conflicting edits

  $ hg up -C 0
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo baz >> file1
  $ echo baz >> file2
  $ hg commit -Am 'append baz to files'
  created new head

create a third head with no conflicting edits
  $ hg up -qC 0
  $ echo foo > file3
  $ hg commit -Am 'add non-conflicting file'
  adding file3
  created new head

failing merge

  $ hg up -qC 2
  $ hg merge --tool=internal:fail 1
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

resolve --all should abort when no merge in progress

  $ hg resolve --all
  abort: resolve command not applicable when not merging
  [255]

resolve -m should abort when no merge in progress

  $ hg resolve -m
  abort: resolve command not applicable when not merging
  [255]

set up conflict-free merge

  $ hg up -qC 3
  $ hg merge 1
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

resolve --all should do nothing in merge without conflicts
  $ hg resolve --all
  (no more unresolved files)

resolve -m should do nothing in merge without conflicts

  $ hg resolve -m
  (no more unresolved files)

get back to conflicting state

  $ hg up -qC 2
  $ hg merge --tool=internal:fail 1
  0 files updated, 0 files merged, 0 files removed, 2 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]

resolve without arguments should suggest --all
  $ hg resolve
  abort: no files or directories specified
  (use --all to remerge all files)
  [255]

resolve --all should re-merge all unresolved files
  $ hg resolve -q --all
  warning: conflicts during merge.
  merging file1 incomplete! (edit conflicts, then use 'hg resolve --mark')
  warning: conflicts during merge.
  merging file2 incomplete! (edit conflicts, then use 'hg resolve --mark')
  [1]
  $ grep '<<<' file1 > /dev/null
  $ grep '<<<' file2 > /dev/null

resolve <file> should re-merge file
  $ echo resolved > file1
  $ hg resolve -q file1
  warning: conflicts during merge.
  merging file1 incomplete! (edit conflicts, then use 'hg resolve --mark')
  [1]
  $ grep '<<<' file1 > /dev/null

resolve <file> should do nothing if 'file' was marked resolved
  $ echo resolved > file1
  $ hg resolve -m file1
  $ hg resolve -q file1
  $ cat file1
  resolved

test crashed merge with empty mergestate

  $ hg up -qC 1
  $ mkdir .hg/merge
  $ touch .hg/merge/state

resolve -l should be empty

  $ hg resolve -l

  $ cd ..
