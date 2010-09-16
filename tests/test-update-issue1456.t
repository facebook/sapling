  $ rm -rf a
  $ hg init a
  $ cd a

  $ echo foo > foo
  $ hg ci -qAm0
  $ chmod +x foo
  $ hg ci -m1
  $ hg co -q 0
  $ echo dirty > foo
  $ hg up -c
  abort: uncommitted local changes
  [255]
  $ hg up -q
  $ cat foo
  dirty
  $ hg st -A
  M foo

Validate update of standalone execute bit change:

  $ hg up -C 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ chmod -x foo
  $ hg ci -m removeexec
  nothing changed
  [1]
  $ hg up -C 0
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg up
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg st

