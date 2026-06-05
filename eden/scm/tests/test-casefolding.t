#require icasefs no-eden

  $ setconfig checkout.use-rust=true
  $ sl debugfsinfo | grep 'case-sensitive:'
  case-sensitive: no

test file addition with bad case

  $ newclientrepo
  $ echo a > a
  $ sl add A
  adding a
  $ sl st
  A a
  $ sl ci -m adda
  $ sl manifest
  a
  $ cd ..

test case collision on rename (issue750)

  $ newclientrepo
  $ echo a > a
  $ sl --debug ci -Am adda
  adding a
  committing files:
  a
  committing manifest
  committing changelog
  committed 07f4944404050f47db2e5c5071e0e84e7a27bba9

Case-changing renames should work:

  $ sl mv a A
  $ sl mv A a
  $ sl st

addremove after case-changing rename has no effect (issue4590)

  $ sl mv a A
  $ sl addremove
  $ sl revert --all
  forgetting A
  undeleting a

test changing case of path components

  $ mkdir D
  $ echo b > D/b
  $ sl ci -Am addb D/b
  $ sl mv D/b d/b
  D/b: not overwriting - file already committed
  (use 'sl rename --amend --mark' to amend the current commit)
  $ sl mv D/b d/c
  $ sl st
  A D/c
  R D/b
  $ mv D temp
  $ mv temp d
  $ sl st
  A D/c
  R D/b
  $ sl revert -aq
  $ rm d/c
  $ echo c > D/c
  $ sl add "glob:**/c"
  adding D/c
  $ sl st
  A D/c
  $ sl ci -m addc "glob:**/c"
  $ sl mv d/b d/e
  moving D/b to D/e
  $ sl st
  A D/e
  R D/b
  $ sl revert -aq
  $ rm d/e
  $ sl mv d/b D/B
  moving D/b to D/B
  $ sl st
  A D/B
  R D/b
  $ cd ..

test case collision between revisions (issue912)

  $ newclientrepo
  $ echo a > a
  $ sl ci -Am add-lower-a
  adding a
  $ sl rm a
  $ sl ci -Am remove-a
  $ echo B > B
  $ echo A > A

on linux hfs keeps the old case stored, force it

  $ mv a aa
  $ mv aa A
  $ sl ci -Am add-upper-A
  adding A
  adding B

used to fail under case insensitive fs

  $ sl up -C 'desc("add-lower-a")'
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ sl up -C tip
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved

no clobbering of untracked files with wrong casing

  $ sl up -r 'desc("add-lower-a")'
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ echo gold > b
  $ sl up tip
  abort: 1 conflicting file changes:
   B
  (commit, shelve, goto --clean to discard all your changes, or goto --merge to merge them)
  [255]
  $ cat b
  gold
  $ rm b

  $ cd ..

issue 3342: file in nested directory causes unexpected abort

  $ newclientrepo

  $ mkdir -p a/B/c/D
  $ echo e > a/B/c/D/e
  $ sl add a/B/c/D/e
  $ sl ci -m 'add e'

issue 4481: revert across case only renames
  $ sl mv a/B/c/D/e a/B/c/d/E
  $ sl ci -m "uppercase E"
  $ echo 'foo' > a/B/c/D/E
  $ sl ci -m 'e content change'
  $ sl revert --all -r .~2
  removing a/B/c/D/E
  adding a/B/c/D/e
  $ find . | sort
  ./a
  ./a/B
  ./a/B/c
  ./a/B/c/D
  ./a/B/c/D/e
  ./a/B/c/D/e.orig

Make sure we can keep removed and untracked file separate.
  $ newclientrepo
  $ touch foo
  $ sl commit -Aqm a
  $ sl rm foo
  $ touch FOO
  $ sl st
  R foo
  ? FOO
