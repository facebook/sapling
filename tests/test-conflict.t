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
  merging a incomplete! (edit conflicts, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]

  $ hg id
  32e80765d7fe+75234512624c+ tip

  $ cat a
  <<<<<<< local: 32e80765d7fe - test: branch2
  something else
  =======
  something
  >>>>>>> other: 75234512624c  - test: branch1

  $ hg status
  M a
  ? a.orig

Verify custom conflict markers

  $ hg up -q --clean .
  $ printf "\n[ui]\nmergemarkertemplate={author} {rev}\n" >> .hg/hgrc

  $ hg merge 1
  merging a
  warning: conflicts during merge.
  merging a incomplete! (edit conflicts, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]

  $ cat a
  <<<<<<< local: test 2
  something else
  =======
  something
  >>>>>>> other: test 1

Verify basic conflict markers

  $ hg up -q --clean .
  $ printf "\n[ui]\nmergemarkers=basic\n" >> .hg/hgrc

  $ hg merge 1
  merging a
  warning: conflicts during merge.
  merging a incomplete! (edit conflicts, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]

  $ cat a
  <<<<<<< local
  something else
  =======
  something
  >>>>>>> other
