  $ "$TESTDIR/hghave" icasefs || exit 80

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
  $ hg mv a A
  A: not overwriting - file exists

'a' used to be removed under windows

  $ test -f a || echo 'a is missing'
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
  $ cd ..
