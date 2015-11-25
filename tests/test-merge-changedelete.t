Test for
b5605d88dc27: Make ui.prompt repeat on "unrecognized response" again
 (issue897)

840e2b315c1f: Fix misleading error and prompts during update/merge
 (issue556)

Make sure HGMERGE doesn't interfere with the test
  $ unset HGMERGE

  $ status() {
  >     echo "--- status ---"
  >     hg st -A file1 file2 file3
  >     echo "--- resolve --list ---"
  >     hg resolve --list file1 file2 file3
  >     echo "--- debugmergestate ---"
  >     hg debugmergestate
  >     for file in file1 file2 file3; do
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
  $ echo 3 > file3
  $ hg ci -Am 'added files'
  adding file1
  adding file2
  adding file3

  $ hg rm file1
  $ echo changed >> file2
  $ echo changed1 >> file3
  $ hg ci -m 'removed file1, changed file2, changed file3'

  $ hg co 0
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ echo changed >> file1
  $ hg rm file2
  $ echo changed2 >> file3
  $ hg ci -m 'changed file1, removed file2, changed file3'
  created new head


Non-interactive merge:

  $ hg merge -y
  local changed file1 which remote deleted
  use (c)hanged version or (d)elete? c
  remote changed file2 which local deleted
  use (c)hanged version or leave (d)eleted? c
  merging file3
  warning: conflicts while merging file3! (edit, then use 'hg resolve --mark')
  1 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]

  $ status
  --- status ---
  M file2
  M file3
  C file1
  --- resolve --list ---
  U file3
  --- debugmergestate ---
  * version 2 records
  local: 13910f48cf7bdb2a0ba6e24b4900e4fdd5739dd4
  other: 10f9a0a634e82080907e62f075ab119cbc565ea6
  file: file3 (record type "F", state "u", hash d5b0a58bc47161b1b8a831084b366f757c4f0b11)
    local path: file3 (flags "")
    ancestor path: file3 (node 2661d26c649684b482d10f91960cc3db683c38b4)
    other path: file3 (node a2644c43e210356772c7772a8674544a62e06beb)
  --- file1 ---
  1
  changed
  --- file2 ---
  2
  changed
  --- file3 ---
  3
  <<<<<<< local: 13910f48cf7b - test: changed file1, removed file2, changed file3
  changed2
  =======
  changed1
  >>>>>>> other: 10f9a0a634e8  - test: removed file1, changed file2, changed file3


Interactive merge:

  $ hg co -C
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved

  $ hg merge --config ui.interactive=true <<EOF
  > c
  > d
  > EOF
  local changed file1 which remote deleted
  use (c)hanged version or (d)elete? c
  remote changed file2 which local deleted
  use (c)hanged version or leave (d)eleted? d
  merging file3
  warning: conflicts while merging file3! (edit, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]

  $ status
  --- status ---
  file2: * (glob)
  M file3
  C file1
  --- resolve --list ---
  U file3
  --- debugmergestate ---
  * version 2 records
  local: 13910f48cf7bdb2a0ba6e24b4900e4fdd5739dd4
  other: 10f9a0a634e82080907e62f075ab119cbc565ea6
  file: file3 (record type "F", state "u", hash d5b0a58bc47161b1b8a831084b366f757c4f0b11)
    local path: file3 (flags "")
    ancestor path: file3 (node 2661d26c649684b482d10f91960cc3db683c38b4)
    other path: file3 (node a2644c43e210356772c7772a8674544a62e06beb)
  --- file1 ---
  1
  changed
  *** file2 does not exist
  --- file3 ---
  3
  <<<<<<< local: 13910f48cf7b - test: changed file1, removed file2, changed file3
  changed2
  =======
  changed1
  >>>>>>> other: 10f9a0a634e8  - test: removed file1, changed file2, changed file3


Interactive merge with bad input:

  $ hg co -C
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg merge --config ui.interactive=true <<EOF
  > foo
  > bar
  > d
  > baz
  > c
  > EOF
  local changed file1 which remote deleted
  use (c)hanged version or (d)elete? foo
  unrecognized response
  local changed file1 which remote deleted
  use (c)hanged version or (d)elete? bar
  unrecognized response
  local changed file1 which remote deleted
  use (c)hanged version or (d)elete? d
  remote changed file2 which local deleted
  use (c)hanged version or leave (d)eleted? baz
  unrecognized response
  remote changed file2 which local deleted
  use (c)hanged version or leave (d)eleted? c
  merging file3
  warning: conflicts while merging file3! (edit, then use 'hg resolve --mark')
  1 files updated, 0 files merged, 1 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]

  $ status
  --- status ---
  M file2
  M file3
  R file1
  --- resolve --list ---
  U file3
  --- debugmergestate ---
  * version 2 records
  local: 13910f48cf7bdb2a0ba6e24b4900e4fdd5739dd4
  other: 10f9a0a634e82080907e62f075ab119cbc565ea6
  file: file3 (record type "F", state "u", hash d5b0a58bc47161b1b8a831084b366f757c4f0b11)
    local path: file3 (flags "")
    ancestor path: file3 (node 2661d26c649684b482d10f91960cc3db683c38b4)
    other path: file3 (node a2644c43e210356772c7772a8674544a62e06beb)
  *** file1 does not exist
  --- file2 ---
  2
  changed
  --- file3 ---
  3
  <<<<<<< local: 13910f48cf7b - test: changed file1, removed file2, changed file3
  changed2
  =======
  changed1
  >>>>>>> other: 10f9a0a634e8  - test: removed file1, changed file2, changed file3


Interactive merge with not enough input:

  $ hg co -C
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved

  $ hg merge --config ui.interactive=true <<EOF
  > d
  > EOF
  local changed file1 which remote deleted
  use (c)hanged version or (d)elete? d
  remote changed file2 which local deleted
  use (c)hanged version or leave (d)eleted? abort: response expected
  [255]

  $ status
  --- status ---
  file2: * (glob)
  C file1
  C file3
  --- resolve --list ---
  --- debugmergestate ---
  no merge state found
  --- file1 ---
  1
  changed
  *** file2 does not exist
  --- file3 ---
  3
  changed2

Non-interactive linear update

  $ hg co -C 0
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo changed >> file1
  $ hg rm file2
  $ hg update 1 -y
  local changed file1 which remote deleted
  use (c)hanged version or (d)elete? c
  remote changed file2 which local deleted
  use (c)hanged version or leave (d)eleted? c
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ status
  --- status ---
  A file1
  C file2
  C file3
  --- resolve --list ---
  --- debugmergestate ---
  no merge state found
  --- file1 ---
  1
  changed
  --- file2 ---
  2
  changed
  --- file3 ---
  3
  changed1
