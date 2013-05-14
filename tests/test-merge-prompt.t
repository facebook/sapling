Test for
b5605d88dc27: Make ui.prompt repeat on "unrecognized response" again
 (issue897)

840e2b315c1f: Fix misleading error and prompts during update/merge
 (issue556)

  $ status() {
  >     echo "--- status ---"
  >     hg st -A file1 file2
  >     for file in file1 file2; do
  >         if [ -f $file ]; then
  >             echo "--- $file ---"
  >             cat $file
  >         else
  >             echo "*** $file does not exist"
  >         fi
  >     done
  > }

  $ hg init

  $ echo 1 > file1
  $ echo 2 > file2
  $ hg ci -Am 'added file1 and file2'
  adding file1
  adding file2

  $ hg rm file1
  $ echo changed >> file2
  $ hg ci -m 'removed file1, changed file2'

  $ hg co 0
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ echo changed >> file1
  $ hg rm file2
  $ hg ci -m 'changed file1, removed file2'
  created new head


Non-interactive merge:

  $ hg merge -y
  local changed file1 which remote deleted
  use (c)hanged version or (d)elete? c
  remote changed file2 which local deleted
  use (c)hanged version or leave (d)eleted? c
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ status
  --- status ---
  M file2
  C file1
  --- file1 ---
  1
  changed
  --- file2 ---
  2
  changed


Interactive merge:

  $ hg co -C
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved

  $ hg merge --config ui.interactive=true <<EOF
  > c
  > d
  > EOF
  local changed file1 which remote deleted
  use (c)hanged version or (d)elete? remote changed file2 which local deleted
  use (c)hanged version or leave (d)eleted? 0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ status
  --- status ---
  file2: * (glob)
  C file1
  --- file1 ---
  1
  changed
  *** file2 does not exist


Interactive merge with bad input:

  $ hg co -C
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg merge --config ui.interactive=true <<EOF
  > foo
  > bar
  > d
  > baz
  > c
  > EOF
  local changed file1 which remote deleted
  use (c)hanged version or (d)elete? unrecognized response
  local changed file1 which remote deleted
  use (c)hanged version or (d)elete? unrecognized response
  local changed file1 which remote deleted
  use (c)hanged version or (d)elete? remote changed file2 which local deleted
  use (c)hanged version or leave (d)eleted? unrecognized response
  remote changed file2 which local deleted
  use (c)hanged version or leave (d)eleted? 1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ status
  --- status ---
  M file2
  R file1
  *** file1 does not exist
  --- file2 ---
  2
  changed


Interactive merge with not enough input:

  $ hg co -C
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved

  $ hg merge --config ui.interactive=true <<EOF
  > d
  > EOF
  local changed file1 which remote deleted
  use (c)hanged version or (d)elete? remote changed file2 which local deleted
  use (c)hanged version or leave (d)eleted? abort: response expected
  [255]

  $ status
  --- status ---
  file2: * (glob)
  C file1
  --- file1 ---
  1
  changed
  *** file2 does not exist

