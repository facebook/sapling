Tests for change/delete conflicts, including:
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

  $ hg init repo
  $ cd repo

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
  use (c)hanged version, (d)elete, or leave (u)nresolved? u
  remote changed file2 which local deleted
  use (c)hanged version, leave (d)eleted, or leave (u)nresolved? u
  merging file3
  warning: conflicts while merging file3! (edit, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 3 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]

  $ status
  --- status ---
  M file2
  M file3
  C file1
  --- resolve --list ---
  U file1
  U file2
  U file3
  --- debugmergestate ---
  * version 2 records
  local: 13910f48cf7bdb2a0ba6e24b4900e4fdd5739dd4
  other: 10f9a0a634e82080907e62f075ab119cbc565ea6
  file extras: file1 (ancestorlinknode = ab57bf49aa276a22d35a473592d4c34b5abc3eff)
  file: file1 (record type "C", state "u", hash 60b27f004e454aca81b0480209cce5081ec52390)
    local path: file1 (flags "")
    ancestor path: file1 (node b8e02f6433738021a065f94175c7cd23db5f05be)
    other path: file1 (node null)
  file extras: file2 (ancestorlinknode = ab57bf49aa276a22d35a473592d4c34b5abc3eff)
  file: file2 (record type "C", state "u", hash null)
    local path: file2 (flags "")
    ancestor path: file2 (node 5d9299349fc01ddd25d0070d149b124d8f10411e)
    other path: file2 (node e7c1328648519852e723de86c0c0525acd779257)
  file extras: file3 (ancestorlinknode = ab57bf49aa276a22d35a473592d4c34b5abc3eff)
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
  1 other heads for branch "default"

  $ hg merge --config ui.interactive=true <<EOF
  > c
  > d
  > EOF
  local changed file1 which remote deleted
  use (c)hanged version, (d)elete, or leave (u)nresolved? c
  remote changed file2 which local deleted
  use (c)hanged version, leave (d)eleted, or leave (u)nresolved? d
  merging file3
  warning: conflicts while merging file3! (edit, then use 'hg resolve --mark')
  0 files updated, 2 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]

  $ status
  --- status ---
  file2: * (glob)
  M file3
  C file1
  --- resolve --list ---
  R file1
  R file2
  U file3
  --- debugmergestate ---
  * version 2 records
  local: 13910f48cf7bdb2a0ba6e24b4900e4fdd5739dd4
  other: 10f9a0a634e82080907e62f075ab119cbc565ea6
  file extras: file1 (ancestorlinknode = ab57bf49aa276a22d35a473592d4c34b5abc3eff)
  file: file1 (record type "C", state "r", hash 60b27f004e454aca81b0480209cce5081ec52390)
    local path: file1 (flags "")
    ancestor path: file1 (node b8e02f6433738021a065f94175c7cd23db5f05be)
    other path: file1 (node null)
  file extras: file2 (ancestorlinknode = ab57bf49aa276a22d35a473592d4c34b5abc3eff)
  file: file2 (record type "C", state "r", hash null)
    local path: file2 (flags "")
    ancestor path: file2 (node 5d9299349fc01ddd25d0070d149b124d8f10411e)
    other path: file2 (node e7c1328648519852e723de86c0c0525acd779257)
  file extras: file3 (ancestorlinknode = ab57bf49aa276a22d35a473592d4c34b5abc3eff)
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
  1 other heads for branch "default"

  $ hg merge --config ui.interactive=true <<EOF
  > foo
  > bar
  > d
  > baz
  > c
  > EOF
  local changed file1 which remote deleted
  use (c)hanged version, (d)elete, or leave (u)nresolved? foo
  unrecognized response
  local changed file1 which remote deleted
  use (c)hanged version, (d)elete, or leave (u)nresolved? bar
  unrecognized response
  local changed file1 which remote deleted
  use (c)hanged version, (d)elete, or leave (u)nresolved? d
  remote changed file2 which local deleted
  use (c)hanged version, leave (d)eleted, or leave (u)nresolved? baz
  unrecognized response
  remote changed file2 which local deleted
  use (c)hanged version, leave (d)eleted, or leave (u)nresolved? c
  merging file3
  warning: conflicts while merging file3! (edit, then use 'hg resolve --mark')
  0 files updated, 1 files merged, 1 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]

  $ status
  --- status ---
  M file2
  M file3
  R file1
  --- resolve --list ---
  R file1
  R file2
  U file3
  --- debugmergestate ---
  * version 2 records
  local: 13910f48cf7bdb2a0ba6e24b4900e4fdd5739dd4
  other: 10f9a0a634e82080907e62f075ab119cbc565ea6
  file extras: file1 (ancestorlinknode = ab57bf49aa276a22d35a473592d4c34b5abc3eff)
  file: file1 (record type "C", state "r", hash 60b27f004e454aca81b0480209cce5081ec52390)
    local path: file1 (flags "")
    ancestor path: file1 (node b8e02f6433738021a065f94175c7cd23db5f05be)
    other path: file1 (node null)
  file extras: file2 (ancestorlinknode = ab57bf49aa276a22d35a473592d4c34b5abc3eff)
  file: file2 (record type "C", state "r", hash null)
    local path: file2 (flags "")
    ancestor path: file2 (node 5d9299349fc01ddd25d0070d149b124d8f10411e)
    other path: file2 (node e7c1328648519852e723de86c0c0525acd779257)
  file extras: file3 (ancestorlinknode = ab57bf49aa276a22d35a473592d4c34b5abc3eff)
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
  1 other heads for branch "default"

  $ hg merge --config ui.interactive=true <<EOF
  > d
  > EOF
  local changed file1 which remote deleted
  use (c)hanged version, (d)elete, or leave (u)nresolved? d
  remote changed file2 which local deleted
  use (c)hanged version, leave (d)eleted, or leave (u)nresolved? 
  merging file3
  warning: conflicts while merging file3! (edit, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 1 files removed, 2 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]

  $ status
  --- status ---
  M file2
  M file3
  R file1
  --- resolve --list ---
  R file1
  U file2
  U file3
  --- debugmergestate ---
  * version 2 records
  local: 13910f48cf7bdb2a0ba6e24b4900e4fdd5739dd4
  other: 10f9a0a634e82080907e62f075ab119cbc565ea6
  file extras: file1 (ancestorlinknode = ab57bf49aa276a22d35a473592d4c34b5abc3eff)
  file: file1 (record type "C", state "r", hash 60b27f004e454aca81b0480209cce5081ec52390)
    local path: file1 (flags "")
    ancestor path: file1 (node b8e02f6433738021a065f94175c7cd23db5f05be)
    other path: file1 (node null)
  file extras: file2 (ancestorlinknode = ab57bf49aa276a22d35a473592d4c34b5abc3eff)
  file: file2 (record type "C", state "u", hash null)
    local path: file2 (flags "")
    ancestor path: file2 (node 5d9299349fc01ddd25d0070d149b124d8f10411e)
    other path: file2 (node e7c1328648519852e723de86c0c0525acd779257)
  file extras: file3 (ancestorlinknode = ab57bf49aa276a22d35a473592d4c34b5abc3eff)
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

Choose local versions of files

  $ hg co -C
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
  1 other heads for branch "default"

  $ hg merge --tool :local
  0 files updated, 3 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ status 2>&1 | tee $TESTTMP/local.status
  --- status ---
  file2: * (glob)
  M file3
  C file1
  --- resolve --list ---
  R file1
  R file2
  R file3
  --- debugmergestate ---
  * version 2 records
  local: 13910f48cf7bdb2a0ba6e24b4900e4fdd5739dd4
  other: 10f9a0a634e82080907e62f075ab119cbc565ea6
  file extras: file1 (ancestorlinknode = ab57bf49aa276a22d35a473592d4c34b5abc3eff)
  file: file1 (record type "C", state "r", hash 60b27f004e454aca81b0480209cce5081ec52390)
    local path: file1 (flags "")
    ancestor path: file1 (node b8e02f6433738021a065f94175c7cd23db5f05be)
    other path: file1 (node null)
  file extras: file2 (ancestorlinknode = ab57bf49aa276a22d35a473592d4c34b5abc3eff)
  file: file2 (record type "C", state "r", hash null)
    local path: file2 (flags "")
    ancestor path: file2 (node 5d9299349fc01ddd25d0070d149b124d8f10411e)
    other path: file2 (node e7c1328648519852e723de86c0c0525acd779257)
  file extras: file3 (ancestorlinknode = ab57bf49aa276a22d35a473592d4c34b5abc3eff)
  file: file3 (record type "F", state "r", hash d5b0a58bc47161b1b8a831084b366f757c4f0b11)
    local path: file3 (flags "")
    ancestor path: file3 (node 2661d26c649684b482d10f91960cc3db683c38b4)
    other path: file3 (node a2644c43e210356772c7772a8674544a62e06beb)
  --- file1 ---
  1
  changed
  *** file2 does not exist
  --- file3 ---
  3
  changed2

Choose other versions of files

  $ hg co -C
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  1 other heads for branch "default"

  $ hg merge --tool :other
  0 files updated, 2 files merged, 1 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ status 2>&1 | tee $TESTTMP/other.status
  --- status ---
  M file2
  M file3
  R file1
  --- resolve --list ---
  R file1
  R file2
  R file3
  --- debugmergestate ---
  * version 2 records
  local: 13910f48cf7bdb2a0ba6e24b4900e4fdd5739dd4
  other: 10f9a0a634e82080907e62f075ab119cbc565ea6
  file extras: file1 (ancestorlinknode = ab57bf49aa276a22d35a473592d4c34b5abc3eff)
  file: file1 (record type "C", state "r", hash 60b27f004e454aca81b0480209cce5081ec52390)
    local path: file1 (flags "")
    ancestor path: file1 (node b8e02f6433738021a065f94175c7cd23db5f05be)
    other path: file1 (node null)
  file extras: file2 (ancestorlinknode = ab57bf49aa276a22d35a473592d4c34b5abc3eff)
  file: file2 (record type "C", state "r", hash null)
    local path: file2 (flags "")
    ancestor path: file2 (node 5d9299349fc01ddd25d0070d149b124d8f10411e)
    other path: file2 (node e7c1328648519852e723de86c0c0525acd779257)
  file extras: file3 (ancestorlinknode = ab57bf49aa276a22d35a473592d4c34b5abc3eff)
  file: file3 (record type "F", state "r", hash d5b0a58bc47161b1b8a831084b366f757c4f0b11)
    local path: file3 (flags "")
    ancestor path: file3 (node 2661d26c649684b482d10f91960cc3db683c38b4)
    other path: file3 (node a2644c43e210356772c7772a8674544a62e06beb)
  *** file1 does not exist
  --- file2 ---
  2
  changed
  --- file3 ---
  3
  changed1

Fail

  $ hg co -C
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
  1 other heads for branch "default"

  $ hg merge --tool :fail
  0 files updated, 0 files merged, 0 files removed, 3 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ status 2>&1 | tee $TESTTMP/fail.status
  --- status ---
  M file2
  M file3
  C file1
  --- resolve --list ---
  U file1
  U file2
  U file3
  --- debugmergestate ---
  * version 2 records
  local: 13910f48cf7bdb2a0ba6e24b4900e4fdd5739dd4
  other: 10f9a0a634e82080907e62f075ab119cbc565ea6
  file extras: file1 (ancestorlinknode = ab57bf49aa276a22d35a473592d4c34b5abc3eff)
  file: file1 (record type "C", state "u", hash 60b27f004e454aca81b0480209cce5081ec52390)
    local path: file1 (flags "")
    ancestor path: file1 (node b8e02f6433738021a065f94175c7cd23db5f05be)
    other path: file1 (node null)
  file extras: file2 (ancestorlinknode = ab57bf49aa276a22d35a473592d4c34b5abc3eff)
  file: file2 (record type "C", state "u", hash null)
    local path: file2 (flags "")
    ancestor path: file2 (node 5d9299349fc01ddd25d0070d149b124d8f10411e)
    other path: file2 (node e7c1328648519852e723de86c0c0525acd779257)
  file extras: file3 (ancestorlinknode = ab57bf49aa276a22d35a473592d4c34b5abc3eff)
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
  changed2

Force prompts with no input (should be similar to :fail)

  $ hg co -C
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  1 other heads for branch "default"

  $ hg merge --config ui.interactive=True --tool :prompt
  local changed file1 which remote deleted
  use (c)hanged version, (d)elete, or leave (u)nresolved? 
  remote changed file2 which local deleted
  use (c)hanged version, leave (d)eleted, or leave (u)nresolved? 
  no tool found to merge file3
  keep (l)ocal, take (o)ther, or leave (u)nresolved? 
  0 files updated, 0 files merged, 0 files removed, 3 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ status 2>&1 | tee $TESTTMP/prompt.status
  --- status ---
  M file2
  M file3
  C file1
  --- resolve --list ---
  U file1
  U file2
  U file3
  --- debugmergestate ---
  * version 2 records
  local: 13910f48cf7bdb2a0ba6e24b4900e4fdd5739dd4
  other: 10f9a0a634e82080907e62f075ab119cbc565ea6
  file extras: file1 (ancestorlinknode = ab57bf49aa276a22d35a473592d4c34b5abc3eff)
  file: file1 (record type "C", state "u", hash 60b27f004e454aca81b0480209cce5081ec52390)
    local path: file1 (flags "")
    ancestor path: file1 (node b8e02f6433738021a065f94175c7cd23db5f05be)
    other path: file1 (node null)
  file extras: file2 (ancestorlinknode = ab57bf49aa276a22d35a473592d4c34b5abc3eff)
  file: file2 (record type "C", state "u", hash null)
    local path: file2 (flags "")
    ancestor path: file2 (node 5d9299349fc01ddd25d0070d149b124d8f10411e)
    other path: file2 (node e7c1328648519852e723de86c0c0525acd779257)
  file extras: file3 (ancestorlinknode = ab57bf49aa276a22d35a473592d4c34b5abc3eff)
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
  changed2
  $ cmp $TESTTMP/fail.status $TESTTMP/prompt.status || diff -U8 $TESTTMP/fail.status $TESTTMP/prompt.status


Force prompts

  $ hg co -C
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  1 other heads for branch "default"

  $ hg merge --tool :prompt
  local changed file1 which remote deleted
  use (c)hanged version, (d)elete, or leave (u)nresolved? u
  remote changed file2 which local deleted
  use (c)hanged version, leave (d)eleted, or leave (u)nresolved? u
  no tool found to merge file3
  keep (l)ocal, take (o)ther, or leave (u)nresolved? u
  0 files updated, 0 files merged, 0 files removed, 3 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ status
  --- status ---
  M file2
  M file3
  C file1
  --- resolve --list ---
  U file1
  U file2
  U file3
  --- debugmergestate ---
  * version 2 records
  local: 13910f48cf7bdb2a0ba6e24b4900e4fdd5739dd4
  other: 10f9a0a634e82080907e62f075ab119cbc565ea6
  file extras: file1 (ancestorlinknode = ab57bf49aa276a22d35a473592d4c34b5abc3eff)
  file: file1 (record type "C", state "u", hash 60b27f004e454aca81b0480209cce5081ec52390)
    local path: file1 (flags "")
    ancestor path: file1 (node b8e02f6433738021a065f94175c7cd23db5f05be)
    other path: file1 (node null)
  file extras: file2 (ancestorlinknode = ab57bf49aa276a22d35a473592d4c34b5abc3eff)
  file: file2 (record type "C", state "u", hash null)
    local path: file2 (flags "")
    ancestor path: file2 (node 5d9299349fc01ddd25d0070d149b124d8f10411e)
    other path: file2 (node e7c1328648519852e723de86c0c0525acd779257)
  file extras: file3 (ancestorlinknode = ab57bf49aa276a22d35a473592d4c34b5abc3eff)
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
  changed2

Choose to merge all files

  $ hg co -C
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  1 other heads for branch "default"

  $ hg merge --tool :merge3
  local changed file1 which remote deleted
  use (c)hanged version, (d)elete, or leave (u)nresolved? u
  remote changed file2 which local deleted
  use (c)hanged version, leave (d)eleted, or leave (u)nresolved? u
  merging file3
  warning: conflicts while merging file3! (edit, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 3 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ status
  --- status ---
  M file2
  M file3
  C file1
  --- resolve --list ---
  U file1
  U file2
  U file3
  --- debugmergestate ---
  * version 2 records
  local: 13910f48cf7bdb2a0ba6e24b4900e4fdd5739dd4
  other: 10f9a0a634e82080907e62f075ab119cbc565ea6
  file extras: file1 (ancestorlinknode = ab57bf49aa276a22d35a473592d4c34b5abc3eff)
  file: file1 (record type "C", state "u", hash 60b27f004e454aca81b0480209cce5081ec52390)
    local path: file1 (flags "")
    ancestor path: file1 (node b8e02f6433738021a065f94175c7cd23db5f05be)
    other path: file1 (node null)
  file extras: file2 (ancestorlinknode = ab57bf49aa276a22d35a473592d4c34b5abc3eff)
  file: file2 (record type "C", state "u", hash null)
    local path: file2 (flags "")
    ancestor path: file2 (node 5d9299349fc01ddd25d0070d149b124d8f10411e)
    other path: file2 (node e7c1328648519852e723de86c0c0525acd779257)
  file extras: file3 (ancestorlinknode = ab57bf49aa276a22d35a473592d4c34b5abc3eff)
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
  ||||||| base
  =======
  changed1
  >>>>>>> other: 10f9a0a634e8  - test: removed file1, changed file2, changed file3

Exercise transitions between local, other, fail and prompt, and make sure the
dirstate stays consistent. (Compare with each other and to the above
invocations.)

  $ testtransitions() {
  >     # this traversal order covers every transition
  >     tools="local other prompt local fail other local prompt other fail prompt fail local"
  >     lasttool="merge3"
  >     for tool in $tools; do
  >         echo "=== :$lasttool -> :$tool ==="
  >         ref="$TESTTMP/$tool.status"
  >         hg resolve --unmark --all
  >         hg resolve --tool ":$tool" --all --config ui.interactive=True
  >         status > "$TESTTMP/compare.status" 2>&1
  >         echo '--- diff of status ---'
  >         if cmp "$TESTTMP/$tool.status" "$TESTTMP/compare.status" || diff -U8 "$TESTTMP/$tool.status" "$TESTTMP/compare.status"; then
  >             echo '(status identical)'
  >         fi
  >         lasttool="$tool"
  >         echo
  >     done
  > }

  $ testtransitions
  === :merge3 -> :local ===
  (no more unresolved files)
  --- diff of status ---
  (status identical)
  
  === :local -> :other ===
  (no more unresolved files)
  --- diff of status ---
  (status identical)
  
  === :other -> :prompt ===
  local changed file1 which remote deleted
  use (c)hanged version, (d)elete, or leave (u)nresolved? 
  remote changed file2 which local deleted
  use (c)hanged version, leave (d)eleted, or leave (u)nresolved? 
  no tool found to merge file3
  keep (l)ocal, take (o)ther, or leave (u)nresolved? 
  --- diff of status ---
  (status identical)
  
  === :prompt -> :local ===
  (no more unresolved files)
  --- diff of status ---
  (status identical)
  
  === :local -> :fail ===
  --- diff of status ---
  (status identical)
  
  === :fail -> :other ===
  (no more unresolved files)
  --- diff of status ---
  (status identical)
  
  === :other -> :local ===
  (no more unresolved files)
  --- diff of status ---
  (status identical)
  
  === :local -> :prompt ===
  local changed file1 which remote deleted
  use (c)hanged version, (d)elete, or leave (u)nresolved? 
  remote changed file2 which local deleted
  use (c)hanged version, leave (d)eleted, or leave (u)nresolved? 
  no tool found to merge file3
  keep (l)ocal, take (o)ther, or leave (u)nresolved? 
  --- diff of status ---
  (status identical)
  
  === :prompt -> :other ===
  (no more unresolved files)
  --- diff of status ---
  (status identical)
  
  === :other -> :fail ===
  --- diff of status ---
  (status identical)
  
  === :fail -> :prompt ===
  local changed file1 which remote deleted
  use (c)hanged version, (d)elete, or leave (u)nresolved? 
  remote changed file2 which local deleted
  use (c)hanged version, leave (d)eleted, or leave (u)nresolved? 
  no tool found to merge file3
  keep (l)ocal, take (o)ther, or leave (u)nresolved? 
  --- diff of status ---
  (status identical)
  
  === :prompt -> :fail ===
  --- diff of status ---
  (status identical)
  
  === :fail -> :local ===
  (no more unresolved files)
  --- diff of status ---
  (status identical)
  


Non-interactive linear update

  $ hg co -C 0
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo changed >> file1
  $ hg rm file2
  $ hg update 1 -y
  local changed file1 which remote deleted
  use (c)hanged version, (d)elete, or leave (u)nresolved? u
  remote changed file2 which local deleted
  use (c)hanged version, leave (d)eleted, or leave (u)nresolved? u
  1 files updated, 0 files merged, 0 files removed, 2 files unresolved
  use 'hg resolve' to retry unresolved file merges
  [1]
  $ status
  --- status ---
  A file1
  C file2
  C file3
  --- resolve --list ---
  U file1
  U file2
  --- debugmergestate ---
  * version 2 records
  local: ab57bf49aa276a22d35a473592d4c34b5abc3eff
  other: 10f9a0a634e82080907e62f075ab119cbc565ea6
  labels:
    local: working copy
    other: destination
  file extras: file1 (ancestorlinknode = ab57bf49aa276a22d35a473592d4c34b5abc3eff)
  file: file1 (record type "C", state "u", hash 60b27f004e454aca81b0480209cce5081ec52390)
    local path: file1 (flags "")
    ancestor path: file1 (node b8e02f6433738021a065f94175c7cd23db5f05be)
    other path: file1 (node null)
  file extras: file2 (ancestorlinknode = ab57bf49aa276a22d35a473592d4c34b5abc3eff)
  file: file2 (record type "C", state "u", hash null)
    local path: file2 (flags "")
    ancestor path: file2 (node 5d9299349fc01ddd25d0070d149b124d8f10411e)
    other path: file2 (node e7c1328648519852e723de86c0c0525acd779257)
  --- file1 ---
  1
  changed
  --- file2 ---
  2
  changed
  --- file3 ---
  3
  changed1

Choose local versions of files

  $ hg co -C 0
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo changed >> file1
  $ hg rm file2
  $ hg update 1 --tool :local
  1 files updated, 2 files merged, 0 files removed, 0 files unresolved
  $ status 2>&1 | tee $TESTTMP/local.status
  --- status ---
  file2: * (glob)
  A file1
  C file3
  --- resolve --list ---
  R file1
  R file2
  --- debugmergestate ---
  * version 2 records
  local: ab57bf49aa276a22d35a473592d4c34b5abc3eff
  other: 10f9a0a634e82080907e62f075ab119cbc565ea6
  labels:
    local: working copy
    other: destination
  file extras: file1 (ancestorlinknode = ab57bf49aa276a22d35a473592d4c34b5abc3eff)
  file: file1 (record type "C", state "r", hash 60b27f004e454aca81b0480209cce5081ec52390)
    local path: file1 (flags "")
    ancestor path: file1 (node b8e02f6433738021a065f94175c7cd23db5f05be)
    other path: file1 (node null)
  file extras: file2 (ancestorlinknode = ab57bf49aa276a22d35a473592d4c34b5abc3eff)
  file: file2 (record type "C", state "r", hash null)
    local path: file2 (flags "")
    ancestor path: file2 (node 5d9299349fc01ddd25d0070d149b124d8f10411e)
    other path: file2 (node e7c1328648519852e723de86c0c0525acd779257)
  --- file1 ---
  1
  changed
  *** file2 does not exist
  --- file3 ---
  3
  changed1

Choose other versions of files

  $ hg co -C 0
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo changed >> file1
  $ hg rm file2
  $ hg update 1 --tool :other
  1 files updated, 1 files merged, 1 files removed, 0 files unresolved
  $ status 2>&1 | tee $TESTTMP/other.status
  --- status ---
  file1: * (glob)
  C file2
  C file3
  --- resolve --list ---
  R file1
  R file2
  --- debugmergestate ---
  * version 2 records
  local: ab57bf49aa276a22d35a473592d4c34b5abc3eff
  other: 10f9a0a634e82080907e62f075ab119cbc565ea6
  labels:
    local: working copy
    other: destination
  file extras: file1 (ancestorlinknode = ab57bf49aa276a22d35a473592d4c34b5abc3eff)
  file: file1 (record type "C", state "r", hash 60b27f004e454aca81b0480209cce5081ec52390)
    local path: file1 (flags "")
    ancestor path: file1 (node b8e02f6433738021a065f94175c7cd23db5f05be)
    other path: file1 (node null)
  file extras: file2 (ancestorlinknode = ab57bf49aa276a22d35a473592d4c34b5abc3eff)
  file: file2 (record type "C", state "r", hash null)
    local path: file2 (flags "")
    ancestor path: file2 (node 5d9299349fc01ddd25d0070d149b124d8f10411e)
    other path: file2 (node e7c1328648519852e723de86c0c0525acd779257)
  *** file1 does not exist
  --- file2 ---
  2
  changed
  --- file3 ---
  3
  changed1

Fail

  $ hg co -C 0
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo changed >> file1
  $ hg rm file2
  $ hg update 1 --tool :fail
  1 files updated, 0 files merged, 0 files removed, 2 files unresolved
  use 'hg resolve' to retry unresolved file merges
  [1]
  $ status 2>&1 | tee $TESTTMP/fail.status
  --- status ---
  A file1
  C file2
  C file3
  --- resolve --list ---
  U file1
  U file2
  --- debugmergestate ---
  * version 2 records
  local: ab57bf49aa276a22d35a473592d4c34b5abc3eff
  other: 10f9a0a634e82080907e62f075ab119cbc565ea6
  labels:
    local: working copy
    other: destination
  file extras: file1 (ancestorlinknode = ab57bf49aa276a22d35a473592d4c34b5abc3eff)
  file: file1 (record type "C", state "u", hash 60b27f004e454aca81b0480209cce5081ec52390)
    local path: file1 (flags "")
    ancestor path: file1 (node b8e02f6433738021a065f94175c7cd23db5f05be)
    other path: file1 (node null)
  file extras: file2 (ancestorlinknode = ab57bf49aa276a22d35a473592d4c34b5abc3eff)
  file: file2 (record type "C", state "u", hash null)
    local path: file2 (flags "")
    ancestor path: file2 (node 5d9299349fc01ddd25d0070d149b124d8f10411e)
    other path: file2 (node e7c1328648519852e723de86c0c0525acd779257)
  --- file1 ---
  1
  changed
  --- file2 ---
  2
  changed
  --- file3 ---
  3
  changed1

Force prompts with no input

  $ hg co -C 0
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo changed >> file1
  $ hg rm file2
  $ hg update 1 --config ui.interactive=True --tool :prompt
  local changed file1 which remote deleted
  use (c)hanged version, (d)elete, or leave (u)nresolved? 
  remote changed file2 which local deleted
  use (c)hanged version, leave (d)eleted, or leave (u)nresolved? 
  1 files updated, 0 files merged, 0 files removed, 2 files unresolved
  use 'hg resolve' to retry unresolved file merges
  [1]
  $ status 2>&1 | tee $TESTTMP/prompt.status
  --- status ---
  A file1
  C file2
  C file3
  --- resolve --list ---
  U file1
  U file2
  --- debugmergestate ---
  * version 2 records
  local: ab57bf49aa276a22d35a473592d4c34b5abc3eff
  other: 10f9a0a634e82080907e62f075ab119cbc565ea6
  labels:
    local: working copy
    other: destination
  file extras: file1 (ancestorlinknode = ab57bf49aa276a22d35a473592d4c34b5abc3eff)
  file: file1 (record type "C", state "u", hash 60b27f004e454aca81b0480209cce5081ec52390)
    local path: file1 (flags "")
    ancestor path: file1 (node b8e02f6433738021a065f94175c7cd23db5f05be)
    other path: file1 (node null)
  file extras: file2 (ancestorlinknode = ab57bf49aa276a22d35a473592d4c34b5abc3eff)
  file: file2 (record type "C", state "u", hash null)
    local path: file2 (flags "")
    ancestor path: file2 (node 5d9299349fc01ddd25d0070d149b124d8f10411e)
    other path: file2 (node e7c1328648519852e723de86c0c0525acd779257)
  --- file1 ---
  1
  changed
  --- file2 ---
  2
  changed
  --- file3 ---
  3
  changed1
  $ cmp $TESTTMP/fail.status $TESTTMP/prompt.status || diff -U8 $TESTTMP/fail.status $TESTTMP/prompt.status

Choose to merge all files

  $ hg co -C 0
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo changed >> file1
  $ hg rm file2
  $ hg update 1 --tool :merge3
  local changed file1 which remote deleted
  use (c)hanged version, (d)elete, or leave (u)nresolved? u
  remote changed file2 which local deleted
  use (c)hanged version, leave (d)eleted, or leave (u)nresolved? u
  1 files updated, 0 files merged, 0 files removed, 2 files unresolved
  use 'hg resolve' to retry unresolved file merges
  [1]
  $ status
  --- status ---
  A file1
  C file2
  C file3
  --- resolve --list ---
  U file1
  U file2
  --- debugmergestate ---
  * version 2 records
  local: ab57bf49aa276a22d35a473592d4c34b5abc3eff
  other: 10f9a0a634e82080907e62f075ab119cbc565ea6
  labels:
    local: working copy
    other: destination
  file extras: file1 (ancestorlinknode = ab57bf49aa276a22d35a473592d4c34b5abc3eff)
  file: file1 (record type "C", state "u", hash 60b27f004e454aca81b0480209cce5081ec52390)
    local path: file1 (flags "")
    ancestor path: file1 (node b8e02f6433738021a065f94175c7cd23db5f05be)
    other path: file1 (node null)
  file extras: file2 (ancestorlinknode = ab57bf49aa276a22d35a473592d4c34b5abc3eff)
  file: file2 (record type "C", state "u", hash null)
    local path: file2 (flags "")
    ancestor path: file2 (node 5d9299349fc01ddd25d0070d149b124d8f10411e)
    other path: file2 (node e7c1328648519852e723de86c0c0525acd779257)
  --- file1 ---
  1
  changed
  --- file2 ---
  2
  changed
  --- file3 ---
  3
  changed1

Test transitions between different merge tools

  $ testtransitions
  === :merge3 -> :local ===
  (no more unresolved files)
  --- diff of status ---
  (status identical)
  
  === :local -> :other ===
  (no more unresolved files)
  --- diff of status ---
  (status identical)
  
  === :other -> :prompt ===
  local changed file1 which remote deleted
  use (c)hanged version, (d)elete, or leave (u)nresolved? 
  remote changed file2 which local deleted
  use (c)hanged version, leave (d)eleted, or leave (u)nresolved? 
  --- diff of status ---
  (status identical)
  
  === :prompt -> :local ===
  (no more unresolved files)
  --- diff of status ---
  (status identical)
  
  === :local -> :fail ===
  --- diff of status ---
  (status identical)
  
  === :fail -> :other ===
  (no more unresolved files)
  --- diff of status ---
  (status identical)
  
  === :other -> :local ===
  (no more unresolved files)
  --- diff of status ---
  (status identical)
  
  === :local -> :prompt ===
  local changed file1 which remote deleted
  use (c)hanged version, (d)elete, or leave (u)nresolved? 
  remote changed file2 which local deleted
  use (c)hanged version, leave (d)eleted, or leave (u)nresolved? 
  --- diff of status ---
  (status identical)
  
  === :prompt -> :other ===
  (no more unresolved files)
  --- diff of status ---
  (status identical)
  
  === :other -> :fail ===
  --- diff of status ---
  (status identical)
  
  === :fail -> :prompt ===
  local changed file1 which remote deleted
  use (c)hanged version, (d)elete, or leave (u)nresolved? 
  remote changed file2 which local deleted
  use (c)hanged version, leave (d)eleted, or leave (u)nresolved? 
  --- diff of status ---
  (status identical)
  
  === :prompt -> :fail ===
  --- diff of status ---
  (status identical)
  
  === :fail -> :local ===
  (no more unresolved files)
  --- diff of status ---
  (status identical)
  
