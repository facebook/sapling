#require icasefs

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
  committing files:
  a
  committing manifest
  committing changelog
  committed changeset 0:07f4944404050f47db2e5c5071e0e84e7a27bba9

Case-changing renames should work:

  $ hg mv a A
  $ hg mv A a
  $ hg st

addremove after case-changing rename has no effect (issue4590)

  $ hg mv a A
  $ hg addremove
  recording removal of a as rename to A (100% similar)
  $ hg revert --all
  forgetting A
  undeleting a

test changing case of path components

  $ mkdir D
  $ echo b > D/b
  $ hg ci -Am addb D/b
  $ hg mv D/b d/b
  D/b: not overwriting - file exists
  $ hg mv D/b d/c
  $ hg st
  A D/c
  R D/b
  $ mv D temp
  $ mv temp d
  $ hg st
  A D/c
  R D/b
  $ hg revert -aq
  $ rm d/c
  $ echo c > D/c
  $ hg add D/c
  $ hg st
  A D/c
  $ hg ci -m addc D/c
  $ hg mv d/b d/e
  moving D/b to D/e (glob)
  $ hg st
  A D/e
  R D/b
  $ hg revert -aq
  $ rm d/e
  $ hg mv d/b D/B
  moving D/b to D/B (glob)
  $ hg st
  A D/B
  R D/b
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
  $ rm a

test that normal file in different case on target context is not
unlinked by largefiles extension.

  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > largefiles=
  > EOF
  $ hg update -q -C 1
  $ hg status -A
  $ echo 'A as largefiles' > A
  $ hg add --large A
  $ hg commit -m '#3'
  created new head
  $ hg manifest -r 3
  .hglf/A
  $ hg manifest -r 0
  a
  $ hg update -q -C 0
  $ hg status -A
  C a
  $ hg update -q -C 3
  $ hg update -q 0

  $ hg up -C -r 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg mv A a
  $ hg diff -g > rename.diff
  $ hg ci -m 'A -> a'
  $ hg up -q '.^'
  $ hg import rename.diff -m "import rename A -> a"
  applying rename.diff
  $ hg st
  ? rename.diff
  $ hg files
  a
  $ find * | sort
  a
  rename.diff

  $ rm rename.diff

  $ cd ..

issue 3342: file in nested directory causes unexpected abort

  $ hg init issue3342
  $ cd issue3342

  $ mkdir -p a/B/c/D
  $ echo e > a/B/c/D/e
  $ hg add a/B/c/D/e
  $ hg ci -m 'add e'

issue 4481: revert across case only renames
  $ hg mv a/B/c/D/e a/B/c/d/E
  $ hg ci -m "uppercase E"
  $ echo 'foo' > a/B/c/D/E
  $ hg ci -m 'e content change'
  $ hg revert --all -r 0
  removing a/B/c/D/E (glob)
  adding a/B/c/D/e (glob)
  $ find * | sort
  a
  a/B
  a/B/c
  a/B/c/D
  a/B/c/D/e
  a/B/c/D/e.orig

  $ cd ..

issue 3340: mq does not handle case changes correctly

in addition to reported case, 'hg qrefresh' is also tested against
case changes.

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "mq=" >> $HGRCPATH

  $ hg init issue3340
  $ cd issue3340

  $ echo a > mIxEdCaSe
  $ hg add mIxEdCaSe
  $ hg commit -m '#0'
  $ hg rename mIxEdCaSe tmp
  $ hg rename tmp MiXeDcAsE
  $ hg status -A
  A MiXeDcAsE
    mIxEdCaSe
  R mIxEdCaSe
  $ hg qnew changecase
  $ hg status -A
  C MiXeDcAsE

  $ hg qpop -a
  popping changecase
  patch queue now empty
  $ hg qnew refresh-casechange
  $ hg status -A
  C mIxEdCaSe
  $ hg rename mIxEdCaSe tmp
  $ hg rename tmp MiXeDcAsE
  $ hg status -A
  A MiXeDcAsE
    mIxEdCaSe
  R mIxEdCaSe
  $ hg qrefresh
  $ hg status -A
  C MiXeDcAsE

  $ hg qpop -a
  popping refresh-casechange
  patch queue now empty
  $ hg qnew refresh-pattern
  $ hg status
  $ echo A > A
  $ hg add
  adding A
  $ hg qrefresh a # issue 3271, qrefresh with file handled case wrong
  $ hg status # empty status means the qrefresh worked

#if osx

We assume anyone running the tests on a case-insensitive volume on OS
X will be using HFS+. If that's not true, this test will fail.

  $ rm A
  >>> open(u'a\u200c'.encode('utf-8'), 'w').write('unicode is fun')
  $ hg status
  M A

#endif

  $ cd ..
