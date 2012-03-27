  $ "$TESTDIR/hghave" icasefs || exit 80

  $ hg debugfs | grep 'case-sensitive:'
  case-sensitive: no

test file addition with bad case

  $ hg init repo1
  $ cd repo1
  $ echo a > a
  $ hg add A
  adding a
  $ hg st
  A a
  $ hg ci -m adda
  $ hg manifest
  a
  $ cd ..

test case collision on rename (issue750)

  $ hg init repo2
  $ cd repo2
  $ echo a > a
  $ hg --debug ci -Am adda
  adding a
  a
  committed changeset 0:07f4944404050f47db2e5c5071e0e84e7a27bba9

Case-changing renames should work:

  $ hg mv a A
  $ hg mv A a
  $ hg st
  $ cd ..

test case collision between revisions (issue912)

  $ hg init repo3
  $ cd repo3
  $ echo a > a
  $ hg ci -Am adda
  adding a
  $ hg rm a
  $ hg ci -Am removea
  $ echo A > A

on linux hfs keeps the old case stored, force it

  $ mv a aa
  $ mv aa A
  $ hg ci -Am addA
  adding A

used to fail under case insensitive fs

  $ hg up -C 0
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg up -C
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved

no clobbering of untracked files with wrong casing

  $ hg up -r null
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo gold > a
  $ hg up
  A: untracked file differs
  abort: untracked files in working directory differ from files in requested revision
  [255]
  $ cat a
  gold

  $ cd ..
