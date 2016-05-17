  $ remove() {
  >     hg rm $@
  >     echo "exit code: $?"
  >     hg st
  >     # do not use ls -R, which recurses in .hg subdirs on Mac OS X 10.5
  >     find . -name .hg -prune -o -type f -print | sort
  >     hg up -C
  > }

  $ cat >> $HGRCPATH <<EOF
  > [progress]
  > disable=False
  > assume-tty = 1
  > delay = 0
  > # set changedelay really large so we don't see nested topics
  > changedelay = 30000
  > format = topic bar number
  > refresh = 0
  > width = 60
  > EOF

  $ hg init a
  $ cd a
  $ echo a > foo

file not managed

  $ remove foo
  \r (no-eol) (esc)
  deleting [===========================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
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
  \r (no-eol) (esc)
  deleting [===========================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  \r (no-eol) (esc)
  skipping [===========================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  not removing bar: file has been marked for add (use forget to undo)
  exit code: 1
  A bar
  ./bar
  ./foo
  \r (no-eol) (esc)
  updating [===========================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

01 state clean, options none

  $ remove foo
  \r (no-eol) (esc)
  deleting [===========================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  exit code: 0
  R foo
  ? bar
  ./bar
  \r (no-eol) (esc)
  updating [===========================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

02 state modified, options none

  $ echo b >> foo
  $ remove foo
  \r (no-eol) (esc)
  deleting [===========================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  \r (no-eol) (esc)
  skipping [===========================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  not removing foo: file is modified (use -f to force removal)
  exit code: 1
  M foo
  ? bar
  ./bar
  ./foo
  \r (no-eol) (esc)
  updating [===========================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

03 state missing, options none

  $ rm foo
  $ remove foo
  \r (no-eol) (esc)
  deleting [===========================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  exit code: 0
  R foo
  ? bar
  ./bar
  \r (no-eol) (esc)
  updating [===========================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

10 state added, options -f

  $ echo b > bar
  $ hg add bar
  $ remove -f bar
  \r (no-eol) (esc)
  deleting [===========================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  exit code: 0
  ? bar
  ./bar
  ./foo
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm bar

11 state clean, options -f

  $ remove -f foo
  \r (no-eol) (esc)
  deleting [===========================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  exit code: 0
  R foo
  \r (no-eol) (esc)
  updating [===========================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

12 state modified, options -f

  $ echo b >> foo
  $ remove -f foo
  \r (no-eol) (esc)
  deleting [===========================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  exit code: 0
  R foo
  \r (no-eol) (esc)
  updating [===========================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

13 state missing, options -f

  $ rm foo
  $ remove -f foo
  \r (no-eol) (esc)
  deleting [===========================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  exit code: 0
  R foo
  \r (no-eol) (esc)
  updating [===========================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

20 state added, options -A

  $ echo b > bar
  $ hg add bar
  $ remove -A bar
  \r (no-eol) (esc)
  deleting [===========================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  \r (no-eol) (esc)
  skipping [===========================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  not removing bar: file still exists
  exit code: 1
  A bar
  ./bar
  ./foo
  \r (no-eol) (esc)
  updating [===========================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

21 state clean, options -A

  $ remove -A foo
  \r (no-eol) (esc)
  deleting [===========================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  \r (no-eol) (esc)
  skipping [===========================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  not removing foo: file still exists
  exit code: 1
  ? bar
  ./bar
  ./foo
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

22 state modified, options -A

  $ echo b >> foo
  $ remove -A foo
  \r (no-eol) (esc)
  deleting [===========================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  \r (no-eol) (esc)
  skipping [===========================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  not removing foo: file still exists
  exit code: 1
  M foo
  ? bar
  ./bar
  ./foo
  \r (no-eol) (esc)
  updating [===========================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

23 state missing, options -A

  $ rm foo
  $ remove -A foo
  \r (no-eol) (esc)
  deleting [===========================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  exit code: 0
  R foo
  ? bar
  ./bar
  \r (no-eol) (esc)
  updating [===========================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

30 state added, options -Af

  $ echo b > bar
  $ hg add bar
  $ remove -Af bar
  \r (no-eol) (esc)
  deleting [===========================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  exit code: 0
  ? bar
  ./bar
  ./foo
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm bar

31 state clean, options -Af

  $ remove -Af foo
  \r (no-eol) (esc)
  deleting [===========================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  exit code: 0
  R foo
  ./foo
  \r (no-eol) (esc)
  updating [===========================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

32 state modified, options -Af

  $ echo b >> foo
  $ remove -Af foo
  \r (no-eol) (esc)
  deleting [===========================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  exit code: 0
  R foo
  ./foo
  \r (no-eol) (esc)
  updating [===========================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

33 state missing, options -Af

  $ rm foo
  $ remove -Af foo
  \r (no-eol) (esc)
  deleting [===========================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  exit code: 0
  R foo
  \r (no-eol) (esc)
  updating [===========================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
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
  \r (no-eol) (esc)
  deleting [===========================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  \r (no-eol) (esc)
  deleting [=====================>                      ] 1/2\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  \r (no-eol) (esc)
  deleting [===========================================>] 2/2\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  removing test/bar (glob)
  removing test/foo (glob)
  exit code: 0
  R test/bar
  R test/foo
  ./foo
  \r (no-eol) (esc)
  updating [===========================================>] 2/2\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

dir, options -f

  $ rm test/bar
  $ remove -f test
  \r (no-eol) (esc)
  deleting [===========================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  \r (no-eol) (esc)
  deleting [=====================>                      ] 1/2\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  \r (no-eol) (esc)
  deleting [===========================================>] 2/2\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  removing test/bar (glob)
  removing test/foo (glob)
  exit code: 0
  R test/bar
  R test/foo
  ./foo
  \r (no-eol) (esc)
  updating [===========================================>] 2/2\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

dir, options -A

  $ rm test/bar
  $ remove -A test
  \r (no-eol) (esc)
  deleting [===========================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  \r (no-eol) (esc)
  skipping [===========================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  \r (no-eol) (esc)
  deleting [===========================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  removing test/bar (glob)
  not removing test/foo: file still exists (glob)
  exit code: 1
  R test/bar
  ./foo
  ./test/foo
  \r (no-eol) (esc)
  updating [===========================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

dir, options -Af

  $ rm test/bar
  $ remove -Af test
  \r (no-eol) (esc)
  deleting [===========================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  \r (no-eol) (esc)
  deleting [=====================>                      ] 1/2\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  \r (no-eol) (esc)
  deleting [===========================================>] 2/2\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  removing test/bar (glob)
  removing test/foo (glob)
  exit code: 0
  R test/bar
  R test/foo
  ./foo
  ./test/foo
  \r (no-eol) (esc)
  updating [===========================================>] 2/2\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

test remove dropping empty trees (issue1861)

  $ mkdir -p issue1861/b/c
  $ echo x > issue1861/x
  $ echo y > issue1861/b/c/y
  $ hg ci -Am add
  adding issue1861/b/c/y
  adding issue1861/x
  $ hg rm issue1861/b
  \r (no-eol) (esc)
  deleting [===========================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  \r (no-eol) (esc)
  deleting [===========================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  removing issue1861/b/c/y (glob)
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
  \r (no-eol) (esc)
  deleting [===========================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  not removing d1: no tracked files
  [1]
  $ hg add d1/a
  $ rm d1/a
  $ hg rm --after d1
  \r (no-eol) (esc)
  deleting [===========================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  \r (no-eol) (esc)
  deleting [===========================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  removing d1/a (glob)

  $ hg rm --after nosuch
  nosuch: * (glob)
  \r (no-eol) (esc)
  deleting [===========================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  [1]
