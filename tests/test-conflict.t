  $ hg init
  $ echo "nothing" > a
  $ hg add a
  $ hg commit -m ancestor
  $ echo "something" > a
  $ hg commit -m branch1
  $ hg co 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo "something else" > a
  $ hg commit -m branch2
  created new head

  $ hg merge 1
  merging a
  warning: conflicts during merge.
  merging a failed!
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]

  $ hg id
  32e80765d7fe+75234512624c+ tip

  $ cat a
  <<<<<<< local
  something else
  =======
  something
  >>>>>>> other

  $ hg status
  M a
  ? a.orig
