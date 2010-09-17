Make sure that the internal merge tools (internal:fail, internal:local, and
internal:other) are used when matched by a merge-pattern in hgrc

Make sure HGMERGE doesn't interfere with the test:

  $ unset HGMERGE

  $ hg init

Initial file contents:

  $ echo "line 1" > f
  $ echo "line 2" >> f
  $ echo "line 3" >> f
  $ hg ci -Am "revision 0"
  adding f

  $ cat f
  line 1
  line 2
  line 3

Branch 1: editing line 1:

  $ sed 's/line 1/first line/' f > f.new
  $ mv f.new f
  $ hg ci -Am "edited first line"

Branch 2: editing line 3:

  $ hg update 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ sed 's/line 3/third line/' f > f.new
  $ mv f.new f
  $ hg ci -Am "edited third line"
  created new head

Merge using internal:fail tool:

  $ echo "[merge-patterns]" > .hg/hgrc
  $ echo "* = internal:fail" >> .hg/hgrc

  $ hg merge
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]

  $ cat f
  line 1
  line 2
  third line

  $ hg stat
  M f

Merge using internal:local tool:

  $ hg update -C 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ sed 's/internal:fail/internal:local/' .hg/hgrc > .hg/hgrc.new
  $ mv .hg/hgrc.new .hg/hgrc

  $ hg merge
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ cat f
  line 1
  line 2
  third line

  $ hg stat
  M f

Merge using internal:other tool:

  $ hg update -C 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ sed 's/internal:local/internal:other/' .hg/hgrc > .hg/hgrc.new
  $ mv .hg/hgrc.new .hg/hgrc

  $ hg merge
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ cat f
  first line
  line 2
  line 3

  $ hg stat
  M f

Merge using default tool:

  $ hg update -C 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm .hg/hgrc

  $ hg merge
  merging f
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ cat f
  first line
  line 2
  third line

  $ hg stat
  M f

