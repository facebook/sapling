#require icasefs

  $ configure modernclient
  $ hg debugfsinfo | grep 'case-sensitive:'
  case-sensitive: no

test file addition with bad case

  $ setconfig devel.segmented-changelog-rev-compat=True
  $ newclientrepo repo1
  $ echo a > a
  $ hg add A
  $ hg st
  A a
  $ hg ci -m adda
  $ hg manifest
  a
  $ cd ..

test case collision on rename (issue750)

  $ newclientrepo repo2
  $ echo a > a
  $ hg --debug ci -Am adda
  adding a
  committing files:
  a
  committing manifest
  committing changelog
  committed 07f4944404050f47db2e5c5071e0e84e7a27bba9

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
  D/b: not overwriting - file already committed
  (hg rename --force to replace the file by recording a rename)
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
  $ hg add "glob:**/c"
  adding d/c (no-fsmonitor !)
  warning: possible case-folding collision for D/c (fsmonitor !)
  adding D/c (fsmonitor !)
  $ hg st
  A d/c (no-fsmonitor !)
  A D/c (fsmonitor !)
  $ hg ci -m addc "glob:**/c"
  $ hg mv d/b d/e
  $ hg st
  A D/e
  R D/b
  $ hg revert -aq
  $ rm d/e
  $ hg mv d/b D/B
  $ hg st
  A D/B
  R D/b
  $ cd ..

test case collision between revisions (issue912)

  $ newclientrepo repo3
  $ echo a > a
  $ hg ci -Am adda
  adding a
  $ hg rm a
  $ hg ci -Am removea
  $ echo B > B
  $ echo A > A

on linux hfs keeps the old case stored, force it

  $ mv a aa
  $ mv aa A
  $ hg ci -Am addA
  adding A
  adding B

used to fail under case insensitive fs

  $ hg up -C 0
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg up -C
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved

no clobbering of untracked files with wrong casing

  $ hg up -r 0
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ echo gold > b
  $ hg up
  B: untracked file differs
  abort: untracked files in working directory differ from files in requested revision
  [255]
  $ cat b
  gold
  $ rm b

  $ cd ..

issue 3342: file in nested directory causes unexpected abort

  $ newclientrepo issue3342

  $ mkdir -p a/B/c/D
  $ echo e > a/B/c/D/e
  $ hg add a/B/c/D/e
  $ hg ci -m 'add e'

issue 4481: revert across case only renames
  $ hg mv a/B/c/D/e a/B/c/d/E
  $ hg ci -m "uppercase E"
  $ echo 'foo' > a/B/c/D/E
  $ hg ci -m 'e content change'
  $ hg revert --all -r .~2
  removing a/B/c/D/E
  adding a/B/c/D/e
  $ find * | sort
  a
  a/B
  a/B/c
  a/B/c/D
  a/B/c/D/e
  a/B/c/D/e.orig
