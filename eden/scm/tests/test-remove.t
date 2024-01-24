#chg-compatible

  $ remove() {
  >     hg rm $@
  >     echo "exit code: $?"
  >     hg st
  >     # do not use ls -R, which recurses in .hg subdirs on Mac OS X 10.5
  >     find . -name .hg -prune -o -type f -print | sort
  >     hg up -C
  > }

  $ configure modernclient
  $ newclientrepo a
  $ echo a > foo

file not managed

  $ remove foo
  not removing foo: file is untracked
  exit code: 1
  ? foo
  ./foo
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg add foo
  $ hg commit -m1

the table cases
00 state added, options none

  $ echo b > bar
  $ hg add bar
  $ remove bar
  not removing bar: file has been marked for add (use 'hg forget' to undo add)
  exit code: 1
  A bar
  ./bar
  ./foo
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

01 state clean, options none

  $ remove foo
  exit code: 0
  R foo
  ? bar
  ./bar
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

02 state modified, options none

  $ echo b >> foo
  $ remove foo
  not removing foo: file is modified (use -f to force removal)
  exit code: 1
  M foo
  ? bar
  ./bar
  ./foo
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

03 state missing, options none

  $ rm foo
  $ remove foo
  exit code: 0
  R foo
  ? bar
  ./bar
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

10 state added, options -f

  $ echo b > bar
  $ hg add bar
  $ remove -f bar
  exit code: 0
  ? bar
  ./bar
  ./foo
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm bar

11 state clean, options -f

  $ remove -f foo
  exit code: 0
  R foo
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

12 state modified, options -f

  $ echo b >> foo
  $ remove -f foo
  exit code: 0
  R foo
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

13 state missing, options -f

  $ rm foo
  $ remove -f foo
  exit code: 0
  R foo
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

20 state added, options --mark

  $ echo b > bar
  $ hg add bar
  $ remove --mark bar
  not removing bar: file still exists
  exit code: 1
  A bar
  ./bar
  ./foo
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

21 state clean, options --mark -v

  $ remove --mark -v foo
  not removing foo: file still exists
  exit code: 1
  ? bar
  ./bar
  ./foo
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

22 state modified, options --mark -v

  $ echo b >> foo
  $ remove --mark -v foo
  not removing foo: file still exists
  exit code: 1
  M foo
  ? bar
  ./bar
  ./foo
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

23 state missing, options --mark

  $ rm foo
  $ remove --mark foo
  exit code: 0
  R foo
  ? bar
  ./bar
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

30 state added, options --mark -f

  $ echo b > bar
  $ hg add bar
  $ remove --mark -f bar
  exit code: 0
  ? bar
  ./bar
  ./foo
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm bar

31 state clean, options --mark -f

  $ remove --mark -f foo
  exit code: 0
  R foo
  ./foo
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

32 state modified, options --mark -f

  $ echo b >> foo
  $ remove --mark -f foo
  exit code: 0
  R foo
  ./foo
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

33 state missing, options --mark -f

  $ rm foo
  $ remove --mark -f foo
  exit code: 0
  R foo
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

test some directory stuff

  $ mkdir test
  $ echo a > test/foo
  $ echo b > test/bar
  $ hg ci -Am2
  adding test/bar
  adding test/foo

dir, options none

  $ rm test/bar
  $ remove test
  removing test/bar
  removing test/foo
  exit code: 0
  R test/bar
  R test/foo
  ./foo
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

dir, options -f

  $ rm test/bar
  $ remove -f test
  removing test/bar
  removing test/foo
  exit code: 0
  R test/bar
  R test/foo
  ./foo
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

dir, options --mark -v

  $ rm test/bar
  $ remove --mark -v test
  removing test/bar
  exit code: 0
  R test/bar
  ./foo
  ./test/foo
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

dir, options --mark <dir>
  $ rm test/bar
  $ remove --mark test
  removing test/bar
  exit code: 0
  R test/bar
  ./foo
  ./test/foo
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

without any files/dirs, options --mark
  $ rm test/bar
  $ remove --mark
  removing test/bar
  exit code: 0
  R test/bar
  ./foo
  ./test/foo
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

dir, options --mark -f

  $ rm test/bar
  $ remove --mark -f test
  removing test/bar
  removing test/foo
  exit code: 0
  R test/bar
  R test/foo
  ./foo
  ./test/foo
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

test remove dropping empty trees (issue1861)

  $ mkdir -p issue1861/b/c
  $ echo x > issue1861/x
  $ echo y > issue1861/b/c/y
  $ hg ci -Am add
  adding issue1861/b/c/y
  adding issue1861/x
  $ hg rm issue1861/b
  removing issue1861/b/c/y
  $ hg ci -m remove
  $ ls issue1861
  x

test that commit does not crash if the user removes a newly added file

  $ touch f1
  $ hg add f1
  $ rm f1
  $ hg ci -A -mx
  removing f1
  nothing changed
  [1]

handling of untracked directories and missing files

  $ mkdir d1
  $ echo a > d1/a
  $ hg rm --mark d1
  not removing d1: no tracked files
  [1]
  $ hg add d1/a
  $ rm d1/a
  $ hg rm --mark d1
  removing d1/a

  $ hg rm --mark nosuch
  nosuch: $ENOENT$
  [1]

handling root path in remove with matcher

  $ newclientrepo
  $ mkdir dir
  $ echo abc > dir/abc.txt
  $ hg ci -m "abc" -Aq
  $ hg remove -f 'glob:**.txt' -X dir
  $ hg remove -f 'glob:**.txt' -I dir
  removing dir/abc.txt
