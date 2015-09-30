#require symlink

https://bz.mercurial-scm.org/1438

  $ hg init

  $ ln -s foo link
  $ hg add link
  $ hg ci -mbad link
  $ hg rm link
  $ hg ci -mok
  $ hg diff -g -r 0:1 > bad.patch

  $ hg up 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg import --no-commit bad.patch
  applying bad.patch

  $ hg status
  R link
  ? bad.patch

