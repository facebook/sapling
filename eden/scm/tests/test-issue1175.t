#chg-compatible
#debugruntest-compatible

  $ setconfig workingcopy.ruststatus=False
  $ disable treemanifest
https://bz.mercurial-scm.org/1175

  $ newrepo repo
  $ touch a
  $ hg ci -Am0
  adding a

  $ hg mv a a1
  $ hg ci -m1

  $ hg co 'desc(0)'
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
  committed 83a687e8a97c80992ba385bbfd766be181bfb1d1

  $ hg verify
  warning: verify does not actually check anything in this repo

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

https://bz.mercurial-scm.org/show_bug.cgi?id=4476

  $ hg init foo
  $ cd foo
  $ touch a && hg ci -Aqm a
  $ hg mv a b
  $ echo b1 >> b
  $ hg ci -Aqm b1
  $ hg up 'desc(a)'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg mv a b
  $ echo b2 >> b
  $ hg ci -Aqm b2
  $ hg graft 'desc(b1)'
  grafting 5974126fad84 "b1"
  merging b
  warning: 1 conflicts while merging b! (edit, then use 'hg resolve --mark')
  abort: unresolved conflicts, can't continue
  (use 'hg resolve' and 'hg graft --continue')
  [255]
  $ echo a > b
  $ echo b3 >> b
  $ hg resolve --mark b
  (no more unresolved files)
  continue: hg graft --continue
  $ hg graft --continue
  grafting 5974126fad84 "b1"
  warning: can't find ancestor for 'b' copied from 'a'!
  $ hg log -f b -T 'changeset:   {node|short}\nsummary:     {desc}\n\n'
  changeset:   376d30ccffc0
  summary:     b1
  
  changeset:   416baaa2e5e4
  summary:     b2
  
  changeset:   3903775176ed
  summary:     a
  


