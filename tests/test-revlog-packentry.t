  $ hg init repo
  $ cd repo

  $ touch foo
  $ hg ci -Am 'add foo'
  adding foo

  $ hg up -C null
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved

this should be stored as a delta against rev 0

  $ echo foo bar baz > foo
  $ hg ci -Am 'add foo again'
  adding foo
  created new head

  $ hg debugindex .hg/store/data/foo.i
     rev    offset  length   base linkrev nodeid       p1           p2
       0         0       0      0       0 b80de5d13875 000000000000 000000000000
       1         0      24      0       1 0376abec49b8 000000000000 000000000000
