  $ "$TESTDIR/hghave" execbit || exit 80

  $ rm -rf a
  $ hg init a
  $ cd a

  $ echo foo > foo
  $ hg ci -qAm0
  $ echo toremove > toremove
  $ echo todelete > todelete
  $ chmod +x foo toremove todelete
  $ hg ci -qAm1

Test that local removed/deleted, remote removed works with flags
  $ hg rm toremove
  $ rm todelete
  $ hg co -q 0

  $ echo dirty > foo
  $ hg up -c
  abort: uncommitted changes
  [255]
  $ hg up -q
  $ cat foo
  dirty
  $ hg st -A
  M foo
  C todelete
  C toremove

Validate update of standalone execute bit change:

  $ hg up -C 0
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ chmod -x foo
  $ hg ci -m removeexec
  nothing changed
  [1]
  $ hg up -C 0
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg up
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg st

  $ cd ..
