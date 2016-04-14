https://bz.mercurial-scm.org/1175

  $ hg init
  $ touch a
  $ hg ci -Am0
  adding a

  $ hg mv a a1
  $ hg ci -m1

  $ hg co 0
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved

  $ hg mv a a2
  $ hg up
  note: possible conflict - a was renamed multiple times to:
   a2
   a1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg ci -m2

  $ touch a
  $ hg ci -Am3
  adding a

  $ hg mv a b
  $ hg ci -Am4 a

  $ hg ci --debug --traceback -Am5 b
  committing files:
  b
  warning: can't find ancestor for 'b' copied from 'a'!
  committing manifest
  committing changelog
  committed changeset 5:83a687e8a97c80992ba385bbfd766be181bfb1d1

  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  4 files, 6 changesets, 4 total revisions

  $ hg export --git tip
  # HG changeset patch
  # User test
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID 83a687e8a97c80992ba385bbfd766be181bfb1d1
  # Parent  1d1625283f71954f21d14c3d44d0ad3c019c597f
  5
  
  diff --git a/b b/b
  new file mode 100644

http://bz.selenic.com/show_bug.cgi?id=4476

  $ hg init foo
  $ cd foo
  $ touch a && hg ci -Aqm a
  $ hg mv a b
  $ echo b1 >> b
  $ hg ci -Aqm b1
  $ hg up 0
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg mv a b
  $ echo b2 >> b
  $ hg ci -Aqm b2
  $ hg graft 1
  grafting 1:5974126fad84 "b1"
  merging b
  warning: conflicts while merging b! (edit, then use 'hg resolve --mark')
  abort: unresolved conflicts, can't continue
  (use 'hg resolve' and 'hg graft --continue')
  [255]
  $ echo a > b
  $ echo b3 >> b
  $ hg resolve --mark b
  (no more unresolved files)
  continue: hg graft --continue
  $ hg graft --continue
  grafting 1:5974126fad84 "b1"
  warning: can't find ancestor for 'b' copied from 'a'!
  $ hg log -f b -T 'changeset:   {rev}:{node|short}\nsummary:     {desc}\n\n'
  changeset:   3:376d30ccffc0
  summary:     b1
  
  changeset:   2:416baaa2e5e4
  summary:     b2
  
  changeset:   0:3903775176ed
  summary:     a
  


