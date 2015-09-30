https://bz.mercurial-scm.org/842

  $ hg init
  $ echo foo > a
  $ hg ci -Ama
  adding a

  $ hg up -r0000
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved

  $ echo bar > a

Should issue new head warning:

  $ hg ci -Amb
  adding a
  created new head

  $ hg up -r0000
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved

  $ echo stuffy > a

Should not issue new head warning:

  $ hg ci -q -Amc

  $ hg up -r0000
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved

  $ echo crap > a
  $ hg branch testing
  marked working directory as branch testing
  (branches are permanent and global, did you want a bookmark?)

Should not issue warning:

  $ hg ci -q -Amd

