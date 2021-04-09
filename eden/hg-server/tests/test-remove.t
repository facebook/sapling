#chg-compatible

  $ remove() {
  >     hg rm $@
  >     echo "exit code: $?"
  >     hg st
  >     # do not use ls -R, which recurses in .hg subdirs on Mac OS X 10.5
  >     find . -name .hg -prune -o -type f -print | sort
  >     hg up -C
  > }

  $ setconfig progress.debug=true

  $ hg init a
  $ cd a
  $ echo a > foo

file not managed

  $ remove foo
  progress: deleting: 1/1 files (100.00%)
  progress: deleting (end)
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
  progress: deleting: 1/1 files (100.00%)
  progress: deleting (end)
  progress: skipping: 1/1 files (100.00%)
  progress: skipping (end)
  not removing bar: file has been marked for add (use 'hg forget' to undo add)
  exit code: 1
  A bar
  ./bar
  ./foo
  progress: updating: bar 1/1 files (100.00%)
  progress: updating (end)
  progress: recording: 1/1 files (100.00%)
  progress: recording (end)
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

01 state clean, options none

  $ remove foo
  progress: deleting: 1/1 files (100.00%)
  progress: deleting (end)
  exit code: 0
  R foo
  ? bar
  ./bar
  progress: updating: foo 1/1 files (100.00%)
  progress: updating (end)
  progress: recording: 1/1 files (100.00%)
  progress: recording (end)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

02 state modified, options none

  $ echo b >> foo
  $ remove foo
  progress: deleting: 1/1 files (100.00%)
  progress: deleting (end)
  progress: skipping: 1/1 files (100.00%)
  progress: skipping (end)
  not removing foo: file is modified (use -f to force removal)
  exit code: 1
  M foo
  ? bar
  ./bar
  ./foo
  progress: updating: foo 1/1 files (100.00%)
  progress: updating (end)
  progress: recording: 1/1 files (100.00%)
  progress: recording (end)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

03 state missing, options none

  $ rm foo
  $ remove foo
  progress: deleting: 1/1 files (100.00%)
  progress: deleting (end)
  exit code: 0
  R foo
  ? bar
  ./bar
  progress: updating: foo 1/1 files (100.00%)
  progress: updating (end)
  progress: recording: 1/1 files (100.00%)
  progress: recording (end)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

10 state added, options -f

  $ echo b > bar
  $ hg add bar
  $ remove -f bar
  progress: deleting: 1/1 files (100.00%)
  progress: deleting (end)
  exit code: 0
  ? bar
  ./bar
  ./foo
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm bar

11 state clean, options -f

  $ remove -f foo
  progress: deleting: 1/1 files (100.00%)
  progress: deleting (end)
  exit code: 0
  R foo
  progress: updating: foo 1/1 files (100.00%)
  progress: updating (end)
  progress: recording: 1/1 files (100.00%)
  progress: recording (end)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

12 state modified, options -f

  $ echo b >> foo
  $ remove -f foo
  progress: deleting: 1/1 files (100.00%)
  progress: deleting (end)
  exit code: 0
  R foo
  progress: updating: foo 1/1 files (100.00%)
  progress: updating (end)
  progress: recording: 1/1 files (100.00%)
  progress: recording (end)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

13 state missing, options -f

  $ rm foo
  $ remove -f foo
  progress: deleting: 1/1 files (100.00%)
  progress: deleting (end)
  exit code: 0
  R foo
  progress: updating: foo 1/1 files (100.00%)
  progress: updating (end)
  progress: recording: 1/1 files (100.00%)
  progress: recording (end)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

20 state added, options -A

  $ echo b > bar
  $ hg add bar
  $ remove -A bar
  progress: deleting: 1/1 files (100.00%)
  progress: deleting (end)
  progress: skipping: 1/1 files (100.00%)
  progress: skipping (end)
  not removing bar: file still exists
  exit code: 1
  A bar
  ./bar
  ./foo
  progress: updating: bar 1/1 files (100.00%)
  progress: updating (end)
  progress: recording: 1/1 files (100.00%)
  progress: recording (end)
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

21 state clean, options -Av

  $ remove -Av foo
  progress: deleting: 1/1 files (100.00%)
  progress: deleting (end)
  progress: skipping: 1/1 files (100.00%)
  progress: skipping (end)
  not removing foo: file still exists
  exit code: 1
  ? bar
  ./bar
  ./foo
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

22 state modified, options -Av

  $ echo b >> foo
  $ remove -Av foo
  progress: deleting: 1/1 files (100.00%)
  progress: deleting (end)
  progress: skipping: 1/1 files (100.00%)
  progress: skipping (end)
  not removing foo: file still exists
  exit code: 1
  M foo
  ? bar
  ./bar
  ./foo
  progress: updating: foo 1/1 files (100.00%)
  progress: updating (end)
  progress: recording: 1/1 files (100.00%)
  progress: recording (end)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

23 state missing, options -A

  $ rm foo
  $ remove -A foo
  progress: deleting: 1/1 files (100.00%)
  progress: deleting (end)
  exit code: 0
  R foo
  ? bar
  ./bar
  progress: updating: foo 1/1 files (100.00%)
  progress: updating (end)
  progress: recording: 1/1 files (100.00%)
  progress: recording (end)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

30 state added, options -Af

  $ echo b > bar
  $ hg add bar
  $ remove -Af bar
  progress: deleting: 1/1 files (100.00%)
  progress: deleting (end)
  exit code: 0
  ? bar
  ./bar
  ./foo
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm bar

31 state clean, options -Af

  $ remove -Af foo
  progress: deleting: 1/1 files (100.00%)
  progress: deleting (end)
  exit code: 0
  R foo
  ./foo
  progress: updating: foo 1/1 files (100.00%)
  progress: updating (end)
  progress: recording: 1/1 files (100.00%)
  progress: recording (end)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

32 state modified, options -Af

  $ echo b >> foo
  $ remove -Af foo
  progress: deleting: 1/1 files (100.00%)
  progress: deleting (end)
  exit code: 0
  R foo
  ./foo
  progress: updating: foo 1/1 files (100.00%)
  progress: updating (end)
  progress: recording: 1/1 files (100.00%)
  progress: recording (end)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

33 state missing, options -Af

  $ rm foo
  $ remove -Af foo
  progress: deleting: 1/1 files (100.00%)
  progress: deleting (end)
  exit code: 0
  R foo
  progress: updating: foo 1/1 files (100.00%)
  progress: updating (end)
  progress: recording: 1/1 files (100.00%)
  progress: recording (end)
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
  progress: deleting: 1/1 files (100.00%)
  progress: deleting (end)
  progress: deleting: 1/2 files (50.00%)
  removing test/bar
  progress: deleting: 2/2 files (100.00%)
  removing test/foo
  progress: deleting (end)
  exit code: 0
  R test/bar
  R test/foo
  ./foo
  progress: updating: test/foo 2/2 files (100.00%)
  progress: updating (end)
  progress: recording: 1/2 files (50.00%)
  progress: recording: 2/2 files (100.00%)
  progress: recording (end)
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

dir, options -f

  $ rm test/bar
  $ remove -f test
  progress: deleting: 1/1 files (100.00%)
  progress: deleting (end)
  progress: deleting: 1/2 files (50.00%)
  removing test/bar
  progress: deleting: 2/2 files (100.00%)
  removing test/foo
  progress: deleting (end)
  exit code: 0
  R test/bar
  R test/foo
  ./foo
  progress: updating: test/foo 2/2 files (100.00%)
  progress: updating (end)
  progress: recording: 1/2 files (50.00%)
  progress: recording: 2/2 files (100.00%)
  progress: recording (end)
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

dir, options -Av

  $ rm test/bar
  $ remove -Av test
  progress: deleting: 1/1 files (100.00%)
  progress: deleting (end)
  progress: skipping: 1/1 files (100.00%)
  progress: skipping (end)
  progress: deleting: 1/1 files (100.00%)
  removing test/bar
  progress: deleting (end)
  not removing test/foo: file still exists
  exit code: 1
  R test/bar
  ./foo
  ./test/foo
  progress: updating: test/bar 1/1 files (100.00%)
  progress: updating (end)
  progress: recording: 1/1 files (100.00%)
  progress: recording (end)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

dir, options -A <dir>
  $ rm test/bar
  $ remove -A test
  progress: deleting: 1/1 files (100.00%)
  progress: deleting (end)
  progress: skipping: 1/1 files (100.00%)
  progress: skipping (end)
  progress: deleting: 1/1 files (100.00%)
  removing test/bar
  progress: deleting (end)
  exit code: 1
  R test/bar
  ./foo
  ./test/foo
  progress: updating: test/bar 1/1 files (100.00%)
  progress: updating (end)
  progress: recording: 1/1 files (100.00%)
  progress: recording (end)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

without any files/dirs, options -A
  $ rm test/bar
  $ remove -A
  progress: skipping: 1/2 files (50.00%)
  progress: skipping: 2/2 files (100.00%)
  progress: skipping (end)
  progress: deleting: 1/1 files (100.00%)
  removing test/bar
  progress: deleting (end)
  exit code: 1
  R test/bar
  ./foo
  ./test/foo
  progress: updating: test/bar 1/1 files (100.00%)
  progress: updating (end)
  progress: recording: 1/1 files (100.00%)
  progress: recording (end)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

dir, options -Af

  $ rm test/bar
  $ remove -Af test
  progress: deleting: 1/1 files (100.00%)
  progress: deleting (end)
  progress: deleting: 1/2 files (50.00%)
  removing test/bar
  progress: deleting: 2/2 files (100.00%)
  removing test/foo
  progress: deleting (end)
  exit code: 0
  R test/bar
  R test/foo
  ./foo
  ./test/foo
  progress: updating: test/foo 2/2 files (100.00%)
  progress: updating (end)
  progress: recording: 1/2 files (50.00%)
  progress: recording: 2/2 files (100.00%)
  progress: recording (end)
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

test remove dropping empty trees (issue1861)

  $ mkdir -p issue1861/b/c
  $ echo x > issue1861/x
  $ echo y > issue1861/b/c/y
  $ hg ci -Am add
  adding issue1861/b/c/y
  adding issue1861/x
  $ hg rm issue1861/b
  progress: deleting: 1/1 files (100.00%)
  progress: deleting (end)
  progress: deleting: 1/1 files (100.00%)
  removing issue1861/b/c/y
  progress: deleting (end)
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
  $ hg rm --after d1
  progress: deleting: 1/1 files (100.00%)
  progress: deleting (end)
  not removing d1: no tracked files
  [1]
  $ hg add d1/a
  $ rm d1/a
  $ hg rm --after d1
  progress: deleting: 1/1 files (100.00%)
  progress: deleting (end)
  progress: deleting: 1/1 files (100.00%)
  removing d1/a
  progress: deleting (end)

  $ hg rm --after nosuch
  nosuch: * (glob)
  progress: deleting: 1/1 files (100.00%)
  progress: deleting (end)
  [1]

handling root path in remove with matcher

  $ newrepo
  $ mkdir dir
  $ echo abc > dir/abc.txt
  $ hg ci -m "abc" -Aq
  $ hg remove -f 'glob:**.txt' -X dir
  progress: deleting: 1/1 files (100.00%)
  progress: deleting (end)
  $ hg remove -f 'glob:**.txt' -I dir
  progress: deleting: 1/1 files (100.00%)
  progress: deleting (end)
  progress: deleting: 1/1 files (100.00%)
  removing dir/abc.txt
  progress: deleting (end)
