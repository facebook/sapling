This file used to contains all largefile tests.
Do not add any new tests in this file as it his already far too long to run.

It contains all the testing of the basic concepts of large file in a single block.

  $ USERCACHE="$TESTTMP/cache"; export USERCACHE
  $ mkdir "${USERCACHE}"
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > largefiles=
  > purge=
  > rebase=
  > transplant=
  > [phases]
  > publish=False
  > [largefiles]
  > minsize=2
  > patterns=glob:**.dat
  > usercache=${USERCACHE}
  > [hooks]
  > precommit=sh -c "echo \\"Invoking status precommit hook\\"; hg status"
  > EOF

Create the repo with a couple of revisions of both large and normal
files.
Test status and dirstate of largefiles and that summary output is correct.

  $ hg init a
  $ cd a
  $ mkdir sub
  $ echo normal1 > normal1
  $ echo normal2 > sub/normal2
  $ echo large1 > large1
  $ echo large2 > sub/large2
  $ hg add normal1 sub/normal2
  $ hg add --large large1 sub/large2
  $ hg commit -m "add files"
  Invoking status precommit hook
  A large1
  A normal1
  A sub/large2
  A sub/normal2
  $ touch large1 sub/large2
  $ sleep 1
  $ hg st
  $ hg debugstate --nodates
  n 644         41 set                 .hglf/large1
  n 644         41 set                 .hglf/sub/large2
  n 644          8 set                 normal1
  n 644          8 set                 sub/normal2
  $ hg debugstate --large --nodates
  n 644          7 set                 large1
  n 644          7 set                 sub/large2
  $ echo normal11 > normal1
  $ echo normal22 > sub/normal2
  $ echo large11 > large1
  $ echo large22 > sub/large2
  $ hg commit -m "edit files"
  Invoking status precommit hook
  M large1
  M normal1
  M sub/large2
  M sub/normal2
  $ hg sum --large
  parent: 1:ce8896473775 tip
   edit files
  branch: default
  commit: (clean)
  update: (current)
  phases: 2 draft (draft)
  largefiles: (no remote repo)

Commit preserved largefile contents.

  $ cat normal1
  normal11
  $ cat large1
  large11
  $ cat sub/normal2
  normal22
  $ cat sub/large2
  large22

Test status, subdir and unknown files

  $ echo unknown > sub/unknown
  $ hg st --all
  ? sub/unknown
  C large1
  C normal1
  C sub/large2
  C sub/normal2
  $ hg st --all sub
  ? sub/unknown
  C sub/large2
  C sub/normal2
  $ rm sub/unknown

Test messages and exit codes for remove warning cases

  $ hg remove -A large1
  not removing large1: file still exists
  [1]
  $ echo 'modified' > large1
  $ hg remove large1
  not removing large1: file is modified (use -f to force removal)
  [1]
  $ echo 'new' > normalnew
  $ hg add normalnew
  $ echo 'new' > largenew
  $ hg add --large normalnew
  normalnew already tracked!
  $ hg remove normalnew largenew
  not removing largenew: file is untracked
  not removing normalnew: file has been marked for add (use forget to undo)
  [1]
  $ rm normalnew largenew
  $ hg up -Cq

Remove both largefiles and normal files.

  $ hg remove normal1 large1
  $ hg status large1
  R large1
  $ hg commit -m "remove files"
  Invoking status precommit hook
  R large1
  R normal1
  $ ls
  sub
  $ echo "testlargefile" > large1-test
  $ hg add --large large1-test
  $ hg st
  A large1-test
  $ hg rm large1-test
  not removing large1-test: file has been marked for add (use forget to undo)
  [1]
  $ hg st
  A large1-test
  $ hg forget large1-test
  $ hg st
  ? large1-test
  $ hg remove large1-test
  not removing large1-test: file is untracked
  [1]
  $ hg forget large1-test
  not removing large1-test: file is already untracked
  [1]
  $ rm large1-test

Copy both largefiles and normal files (testing that status output is correct).

  $ hg cp sub/normal2 normal1
  $ hg cp sub/large2 large1
  $ hg commit -m "copy files"
  Invoking status precommit hook
  A large1
  A normal1
  $ cat normal1
  normal22
  $ cat large1
  large22

Test moving largefiles and verify that normal files are also unaffected.

  $ hg mv normal1 normal3
  $ hg mv large1 large3
  $ hg mv sub/normal2 sub/normal4
  $ hg mv sub/large2 sub/large4
  $ hg commit -m "move files"
  Invoking status precommit hook
  A large3
  A normal3
  A sub/large4
  A sub/normal4
  R large1
  R normal1
  R sub/large2
  R sub/normal2
  $ cat normal3
  normal22
  $ cat large3
  large22
  $ cat sub/normal4
  normal22
  $ cat sub/large4
  large22


#if serve
Test display of largefiles in hgweb

  $ hg serve -d -p $HGPORT --pid-file ../hg.pid
  $ cat ../hg.pid >> $DAEMON_PIDS
  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'file/tip/?style=raw'
  200 Script output follows
  
  
  drwxr-xr-x sub
  -rw-r--r-- 41 large3
  -rw-r--r-- 9 normal3
  
  
  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT 'file/tip/sub/?style=raw'
  200 Script output follows
  
  
  -rw-r--r-- 41 large4
  -rw-r--r-- 9 normal4
  
  
  $ "$TESTDIR/killdaemons.py" $DAEMON_PIDS
#endif

Test archiving the various revisions.  These hit corner cases known with
archiving.

  $ hg archive -r 0 ../archive0
  $ hg archive -r 1 ../archive1
  $ hg archive -r 2 ../archive2
  $ hg archive -r 3 ../archive3
  $ hg archive -r 4 ../archive4
  $ cd ../archive0
  $ cat normal1
  normal1
  $ cat large1
  large1
  $ cat sub/normal2
  normal2
  $ cat sub/large2
  large2
  $ cd ../archive1
  $ cat normal1
  normal11
  $ cat large1
  large11
  $ cat sub/normal2
  normal22
  $ cat sub/large2
  large22
  $ cd ../archive2
  $ ls
  sub
  $ cat sub/normal2
  normal22
  $ cat sub/large2
  large22
  $ cd ../archive3
  $ cat normal1
  normal22
  $ cat large1
  large22
  $ cat sub/normal2
  normal22
  $ cat sub/large2
  large22
  $ cd ../archive4
  $ cat normal3
  normal22
  $ cat large3
  large22
  $ cat sub/normal4
  normal22
  $ cat sub/large4
  large22

Commit corner case: specify files to commit.

  $ cd ../a
  $ echo normal3 > normal3
  $ echo large3 > large3
  $ echo normal4 > sub/normal4
  $ echo large4 > sub/large4
  $ hg commit normal3 large3 sub/normal4 sub/large4 -m "edit files again"
  Invoking status precommit hook
  M large3
  M normal3
  M sub/large4
  M sub/normal4
  $ cat normal3
  normal3
  $ cat large3
  large3
  $ cat sub/normal4
  normal4
  $ cat sub/large4
  large4

One more commit corner case: commit from a subdirectory.

  $ cd ../a
  $ echo normal33 > normal3
  $ echo large33 > large3
  $ echo normal44 > sub/normal4
  $ echo large44 > sub/large4
  $ cd sub
  $ hg commit -m "edit files yet again"
  Invoking status precommit hook
  M large3
  M normal3
  M sub/large4
  M sub/normal4
  $ cat ../normal3
  normal33
  $ cat ../large3
  large33
  $ cat normal4
  normal44
  $ cat large4
  large44

Committing standins is not allowed.

  $ cd ..
  $ echo large3 > large3
  $ hg commit .hglf/large3 -m "try to commit standin"
  abort: file ".hglf/large3" is a largefile standin
  (commit the largefile itself instead)
  [255]

Corner cases for adding largefiles.

  $ echo large5 > large5
  $ hg add --large large5
  $ hg add --large large5
  large5 already a largefile
  $ mkdir sub2
  $ echo large6 > sub2/large6
  $ echo large7 > sub2/large7
  $ hg add --large sub2
  adding sub2/large6 as a largefile (glob)
  adding sub2/large7 as a largefile (glob)
  $ hg st
  M large3
  A large5
  A sub2/large6
  A sub2/large7

Committing directories containing only largefiles.

  $ mkdir -p z/y/x/m
  $ touch z/y/x/m/large1
  $ touch z/y/x/large2
  $ hg add --large z/y/x/m/large1 z/y/x/large2
  $ hg commit -m "Subdir with directory only containing largefiles" z
  Invoking status precommit hook
  M large3
  A large5
  A sub2/large6
  A sub2/large7
  A z/y/x/large2
  A z/y/x/m/large1

(and a bit of log testing)

  $ hg log -T '{rev}\n' z/y/x/m/large1
  7
  $ hg log -T '{rev}\n' z/y/x/m  # with only a largefile
  7

  $ hg rollback --quiet
  $ touch z/y/x/m/normal
  $ hg add z/y/x/m/normal
  $ hg commit -m "Subdir with mixed contents" z
  Invoking status precommit hook
  M large3
  A large5
  A sub2/large6
  A sub2/large7
  A z/y/x/large2
  A z/y/x/m/large1
  A z/y/x/m/normal
  $ hg st
  M large3
  A large5
  A sub2/large6
  A sub2/large7
  $ hg rollback --quiet
  $ hg revert z/y/x/large2 z/y/x/m/large1
  $ rm z/y/x/large2 z/y/x/m/large1
  $ hg commit -m "Subdir with normal contents" z
  Invoking status precommit hook
  M large3
  A large5
  A sub2/large6
  A sub2/large7
  A z/y/x/m/normal
  $ hg st
  M large3
  A large5
  A sub2/large6
  A sub2/large7
  $ hg rollback --quiet
  $ hg revert --quiet z
  $ hg commit -m "Empty subdir" z
  abort: z: no match under directory!
  [255]
  $ rm -rf z
  $ hg ci -m "standin" .hglf
  abort: file ".hglf" is a largefile standin
  (commit the largefile itself instead)
  [255]

Test "hg status" with combination of 'file pattern' and 'directory
pattern' for largefiles:

  $ hg status sub2/large6 sub2
  A sub2/large6
  A sub2/large7

Config settings (pattern **.dat, minsize 2 MB) are respected.

  $ echo testdata > test.dat
  $ dd bs=1k count=2k if=/dev/zero of=reallylarge > /dev/null 2> /dev/null
  $ hg add
  adding reallylarge as a largefile
  adding test.dat as a largefile

Test that minsize and --lfsize handle float values;
also tests that --lfsize overrides largefiles.minsize.
(0.250 MB = 256 kB = 262144 B)

  $ dd if=/dev/zero of=ratherlarge bs=1024 count=256 > /dev/null 2> /dev/null
  $ dd if=/dev/zero of=medium bs=1024 count=128 > /dev/null 2> /dev/null
  $ hg --config largefiles.minsize=.25 add
  adding ratherlarge as a largefile
  adding medium
  $ hg forget medium
  $ hg --config largefiles.minsize=.25 add --lfsize=.125
  adding medium as a largefile
  $ dd if=/dev/zero of=notlarge bs=1024 count=127 > /dev/null 2> /dev/null
  $ hg --config largefiles.minsize=.25 add --lfsize=.125
  adding notlarge
  $ hg forget notlarge

Test forget on largefiles.

  $ hg forget large3 large5 test.dat reallylarge ratherlarge medium
  $ hg commit -m "add/edit more largefiles"
  Invoking status precommit hook
  A sub2/large6
  A sub2/large7
  R large3
  ? large5
  ? medium
  ? notlarge
  ? ratherlarge
  ? reallylarge
  ? test.dat
  $ hg st
  ? large3
  ? large5
  ? medium
  ? notlarge
  ? ratherlarge
  ? reallylarge
  ? test.dat

Purge with largefiles: verify that largefiles are still in the working
dir after a purge.

  $ hg purge --all
  $ cat sub/large4
  large44
  $ cat sub2/large6
  large6
  $ cat sub2/large7
  large7

Test addremove: verify that files that should be added as largefiles are added as
such and that already-existing largefiles are not added as normal files by
accident.

  $ rm normal3
  $ rm sub/large4
  $ echo "testing addremove with patterns" > testaddremove.dat
  $ echo "normaladdremove" > normaladdremove
  $ hg addremove
  removing sub/large4
  adding testaddremove.dat as a largefile
  removing normal3
  adding normaladdremove

Test addremove with -R

  $ hg up -C
  getting changed largefiles
  1 largefiles updated, 0 removed
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm normal3
  $ rm sub/large4
  $ echo "testing addremove with patterns" > testaddremove.dat
  $ echo "normaladdremove" > normaladdremove
  $ cd ..
  $ hg -R a -v addremove
  removing sub/large4
  adding testaddremove.dat as a largefile
  removing normal3
  adding normaladdremove
  $ cd a

Test 3364
  $ hg clone . ../addrm
  updating to branch default
  getting changed largefiles
  3 largefiles updated, 0 removed
  5 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ../addrm
  $ cat >> .hg/hgrc <<EOF
  > [hooks]
  > post-commit.stat=sh -c "echo \\"Invoking status postcommit hook\\"; hg status -A"
  > EOF
  $ touch foo
  $ hg add --large foo
  $ hg ci -m "add foo"
  Invoking status precommit hook
  A foo
  Invoking status postcommit hook
  C foo
  C normal3
  C sub/large4
  C sub/normal4
  C sub2/large6
  C sub2/large7
  $ rm foo
  $ hg st
  ! foo
hmm.. no precommit invoked, but there is a postcommit??
  $ hg ci -m "will not checkin"
  nothing changed
  Invoking status postcommit hook
  ! foo
  C normal3
  C sub/large4
  C sub/normal4
  C sub2/large6
  C sub2/large7
  [1]
  $ hg addremove
  removing foo
  $ hg st
  R foo
  $ hg ci -m "used to say nothing changed"
  Invoking status precommit hook
  R foo
  Invoking status postcommit hook
  C normal3
  C sub/large4
  C sub/normal4
  C sub2/large6
  C sub2/large7
  $ hg st

Test 3507 (both normal files and largefiles were a problem)

  $ touch normal
  $ touch large
  $ hg add normal
  $ hg add --large large
  $ hg ci -m "added"
  Invoking status precommit hook
  A large
  A normal
  Invoking status postcommit hook
  C large
  C normal
  C normal3
  C sub/large4
  C sub/normal4
  C sub2/large6
  C sub2/large7
  $ hg remove normal
  $ hg addremove --traceback
  $ hg ci -m "addremoved normal"
  Invoking status precommit hook
  R normal
  Invoking status postcommit hook
  C large
  C normal3
  C sub/large4
  C sub/normal4
  C sub2/large6
  C sub2/large7
  $ hg up -C '.^'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg remove large
  $ hg addremove --traceback
  $ hg ci -m "removed large"
  Invoking status precommit hook
  R large
  created new head
  Invoking status postcommit hook
  C normal
  C normal3
  C sub/large4
  C sub/normal4
  C sub2/large6
  C sub2/large7

Test commit -A (issue3542)
  $ echo large8 > large8
  $ hg add --large large8
  $ hg ci -Am 'this used to add large8 as normal and commit both'
  Invoking status precommit hook
  A large8
  Invoking status postcommit hook
  C large8
  C normal
  C normal3
  C sub/large4
  C sub/normal4
  C sub2/large6
  C sub2/large7
  $ rm large8
  $ hg ci -Am 'this used to not notice the rm'
  removing large8
  Invoking status precommit hook
  R large8
  Invoking status postcommit hook
  C normal
  C normal3
  C sub/large4
  C sub/normal4
  C sub2/large6
  C sub2/large7

Test that a standin can't be added as a large file

  $ touch large
  $ hg add --large large
  $ hg ci -m "add"
  Invoking status precommit hook
  A large
  Invoking status postcommit hook
  C large
  C normal
  C normal3
  C sub/large4
  C sub/normal4
  C sub2/large6
  C sub2/large7
  $ hg remove large
  $ touch large
  $ hg addremove --config largefiles.patterns=**large --traceback
  adding large as a largefile

Test that outgoing --large works (with revsets too)
  $ hg outgoing --rev '.^' --large
  comparing with $TESTTMP/a (glob)
  searching for changes
  changeset:   8:c02fd3b77ec4
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add foo
  
  changeset:   9:289dd08c9bbb
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     used to say nothing changed
  
  changeset:   10:34f23ac6ac12
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     added
  
  changeset:   12:710c1b2f523c
  parent:      10:34f23ac6ac12
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     removed large
  
  changeset:   13:0a3e75774479
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     this used to add large8 as normal and commit both
  
  changeset:   14:84f3d378175c
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     this used to not notice the rm
  
  largefiles to upload (1 entities):
  large8
  
  $ cd ../a

Clone a largefiles repo.

  $ hg clone . ../b
  updating to branch default
  getting changed largefiles
  3 largefiles updated, 0 removed
  5 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ../b
  $ hg log --template '{rev}:{node|short}  {desc|firstline}\n'
  7:daea875e9014  add/edit more largefiles
  6:4355d653f84f  edit files yet again
  5:9d5af5072dbd  edit files again
  4:74c02385b94c  move files
  3:9e8fbc4bce62  copy files
  2:51a0ae4d5864  remove files
  1:ce8896473775  edit files
  0:30d30fe6a5be  add files
  $ cat normal3
  normal33

Test graph log

  $ hg log -G --template '{rev}:{node|short}  {desc|firstline}\n'
  @  7:daea875e9014  add/edit more largefiles
  |
  o  6:4355d653f84f  edit files yet again
  |
  o  5:9d5af5072dbd  edit files again
  |
  o  4:74c02385b94c  move files
  |
  o  3:9e8fbc4bce62  copy files
  |
  o  2:51a0ae4d5864  remove files
  |
  o  1:ce8896473775  edit files
  |
  o  0:30d30fe6a5be  add files
  

Test log with --patch

  $ hg log --patch -r 6::7
  changeset:   6:4355d653f84f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     edit files yet again
  
  diff -r 9d5af5072dbd -r 4355d653f84f .hglf/large3
  --- a/.hglf/large3	Thu Jan 01 00:00:00 1970 +0000
  +++ b/.hglf/large3	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -baaf12afde9d8d67f25dab6dced0d2bf77dba47c
  +7838695e10da2bb75ac1156565f40a2595fa2fa0
  diff -r 9d5af5072dbd -r 4355d653f84f .hglf/sub/large4
  --- a/.hglf/sub/large4	Thu Jan 01 00:00:00 1970 +0000
  +++ b/.hglf/sub/large4	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -aeb2210d19f02886dde00dac279729a48471e2f9
  +971fb41e78fea4f8e0ba5244784239371cb00591
  diff -r 9d5af5072dbd -r 4355d653f84f normal3
  --- a/normal3	Thu Jan 01 00:00:00 1970 +0000
  +++ b/normal3	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -normal3
  +normal33
  diff -r 9d5af5072dbd -r 4355d653f84f sub/normal4
  --- a/sub/normal4	Thu Jan 01 00:00:00 1970 +0000
  +++ b/sub/normal4	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -normal4
  +normal44
  
  changeset:   7:daea875e9014
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add/edit more largefiles
  
  diff -r 4355d653f84f -r daea875e9014 .hglf/large3
  --- a/.hglf/large3	Thu Jan 01 00:00:00 1970 +0000
  +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +0,0 @@
  -7838695e10da2bb75ac1156565f40a2595fa2fa0
  diff -r 4355d653f84f -r daea875e9014 .hglf/sub2/large6
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/.hglf/sub2/large6	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +0d6d75887db61b2c7e6c74b5dd8fc6ad50c0cc30
  diff -r 4355d653f84f -r daea875e9014 .hglf/sub2/large7
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/.hglf/sub2/large7	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +bb3151689acb10f0c3125c560d5e63df914bc1af
  

  $ hg log --patch -r 6::7 sub/
  changeset:   6:4355d653f84f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     edit files yet again
  
  diff -r 9d5af5072dbd -r 4355d653f84f .hglf/sub/large4
  --- a/.hglf/sub/large4	Thu Jan 01 00:00:00 1970 +0000
  +++ b/.hglf/sub/large4	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -aeb2210d19f02886dde00dac279729a48471e2f9
  +971fb41e78fea4f8e0ba5244784239371cb00591
  diff -r 9d5af5072dbd -r 4355d653f84f sub/normal4
  --- a/sub/normal4	Thu Jan 01 00:00:00 1970 +0000
  +++ b/sub/normal4	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -normal4
  +normal44
  

log with both --follow and --patch

  $ hg log --follow --patch --limit 2
  changeset:   7:daea875e9014
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add/edit more largefiles
  
  diff -r 4355d653f84f -r daea875e9014 .hglf/large3
  --- a/.hglf/large3	Thu Jan 01 00:00:00 1970 +0000
  +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +0,0 @@
  -7838695e10da2bb75ac1156565f40a2595fa2fa0
  diff -r 4355d653f84f -r daea875e9014 .hglf/sub2/large6
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/.hglf/sub2/large6	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +0d6d75887db61b2c7e6c74b5dd8fc6ad50c0cc30
  diff -r 4355d653f84f -r daea875e9014 .hglf/sub2/large7
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/.hglf/sub2/large7	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +bb3151689acb10f0c3125c560d5e63df914bc1af
  
  changeset:   6:4355d653f84f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     edit files yet again
  
  diff -r 9d5af5072dbd -r 4355d653f84f .hglf/large3
  --- a/.hglf/large3	Thu Jan 01 00:00:00 1970 +0000
  +++ b/.hglf/large3	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -baaf12afde9d8d67f25dab6dced0d2bf77dba47c
  +7838695e10da2bb75ac1156565f40a2595fa2fa0
  diff -r 9d5af5072dbd -r 4355d653f84f .hglf/sub/large4
  --- a/.hglf/sub/large4	Thu Jan 01 00:00:00 1970 +0000
  +++ b/.hglf/sub/large4	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -aeb2210d19f02886dde00dac279729a48471e2f9
  +971fb41e78fea4f8e0ba5244784239371cb00591
  diff -r 9d5af5072dbd -r 4355d653f84f normal3
  --- a/normal3	Thu Jan 01 00:00:00 1970 +0000
  +++ b/normal3	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -normal3
  +normal33
  diff -r 9d5af5072dbd -r 4355d653f84f sub/normal4
  --- a/sub/normal4	Thu Jan 01 00:00:00 1970 +0000
  +++ b/sub/normal4	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -normal4
  +normal44
  
  $ hg log --follow --patch sub/large4
  changeset:   6:4355d653f84f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     edit files yet again
  
  diff -r 9d5af5072dbd -r 4355d653f84f .hglf/sub/large4
  --- a/.hglf/sub/large4	Thu Jan 01 00:00:00 1970 +0000
  +++ b/.hglf/sub/large4	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -aeb2210d19f02886dde00dac279729a48471e2f9
  +971fb41e78fea4f8e0ba5244784239371cb00591
  
  changeset:   5:9d5af5072dbd
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     edit files again
  
  diff -r 74c02385b94c -r 9d5af5072dbd .hglf/sub/large4
  --- a/.hglf/sub/large4	Thu Jan 01 00:00:00 1970 +0000
  +++ b/.hglf/sub/large4	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -eb7338044dc27f9bc59b8dd5a246b065ead7a9c4
  +aeb2210d19f02886dde00dac279729a48471e2f9
  
  changeset:   4:74c02385b94c
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     move files
  
  diff -r 9e8fbc4bce62 -r 74c02385b94c .hglf/sub/large4
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/.hglf/sub/large4	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +eb7338044dc27f9bc59b8dd5a246b065ead7a9c4
  
  changeset:   1:ce8896473775
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     edit files
  
  diff -r 30d30fe6a5be -r ce8896473775 .hglf/sub/large2
  --- a/.hglf/sub/large2	Thu Jan 01 00:00:00 1970 +0000
  +++ b/.hglf/sub/large2	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -1deebade43c8c498a3c8daddac0244dc55d1331d
  +eb7338044dc27f9bc59b8dd5a246b065ead7a9c4
  
  changeset:   0:30d30fe6a5be
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add files
  
  diff -r 000000000000 -r 30d30fe6a5be .hglf/sub/large2
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/.hglf/sub/large2	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +1deebade43c8c498a3c8daddac0244dc55d1331d
  
  $ cat sub/normal4
  normal44
  $ cat sub/large4
  large44
  $ cat sub2/large6
  large6
  $ cat sub2/large7
  large7
  $ hg log -qf sub2/large7
  7:daea875e9014
  $ hg log -Gqf sub2/large7
  @  7:daea875e9014
  |
  $ cd ..

Test log from outside repo

  $ hg log  b/sub -T '{rev}:{node|short}  {desc|firstline}\n'
  6:4355d653f84f  edit files yet again
  5:9d5af5072dbd  edit files again
  4:74c02385b94c  move files
  1:ce8896473775  edit files
  0:30d30fe6a5be  add files

Test clone at revision

  $ hg clone a -r 3 c
  adding changesets
  adding manifests
  adding file changes
  added 4 changesets with 10 changes to 4 files
  updating to branch default
  getting changed largefiles
  2 largefiles updated, 0 removed
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd c
  $ hg log --template '{rev}:{node|short}  {desc|firstline}\n'
  3:9e8fbc4bce62  copy files
  2:51a0ae4d5864  remove files
  1:ce8896473775  edit files
  0:30d30fe6a5be  add files
  $ cat normal1
  normal22
  $ cat large1
  large22
  $ cat sub/normal2
  normal22
  $ cat sub/large2
  large22

Old revisions of a clone have correct largefiles content (this also
tests update).

  $ hg update -r 1
  getting changed largefiles
  1 largefiles updated, 0 removed
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat large1
  large11
  $ cat sub/large2
  large22
  $ cd ..

Test cloning with --all-largefiles flag

  $ rm "${USERCACHE}"/*
  $ hg clone --all-largefiles a a-backup
  updating to branch default
  getting changed largefiles
  3 largefiles updated, 0 removed
  5 files updated, 0 files merged, 0 files removed, 0 files unresolved
  8 additional largefiles cached

  $ rm "${USERCACHE}"/*
  $ hg clone --all-largefiles -u 0 a a-clone0
  updating to branch default
  getting changed largefiles
  2 largefiles updated, 0 removed
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  9 additional largefiles cached
  $ hg -R a-clone0 sum
  parent: 0:30d30fe6a5be 
   add files
  branch: default
  commit: (clean)
  update: 7 new changesets (update)
  phases: 8 draft (draft)

  $ rm "${USERCACHE}"/*
  $ hg clone --all-largefiles -u 1 a a-clone1
  updating to branch default
  getting changed largefiles
  2 largefiles updated, 0 removed
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  8 additional largefiles cached
  $ hg -R a-clone1 verify --large --lfa --lfc
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  10 files, 8 changesets, 24 total revisions
  searching 8 changesets for largefiles
  verified contents of 13 revisions of 6 largefiles
  $ hg -R a-clone1 sum
  parent: 1:ce8896473775 
   edit files
  branch: default
  commit: (clean)
  update: 6 new changesets (update)
  phases: 8 draft (draft)

  $ rm "${USERCACHE}"/*
  $ hg clone --all-largefiles -U a a-clone-u
  11 additional largefiles cached
  $ hg -R a-clone-u sum
  parent: -1:000000000000  (no revision checked out)
  branch: default
  commit: (clean)
  update: 8 new changesets (update)
  phases: 8 draft (public)

Show computed destination directory:

  $ mkdir xyz
  $ cd xyz
  $ hg clone ../a
  destination directory: a
  updating to branch default
  getting changed largefiles
  3 largefiles updated, 0 removed
  5 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ..

Clone URL without path:

  $ hg clone file://
  abort: repository / not found!
  [255]

Ensure base clone command argument validation

  $ hg clone -U -u 0 a a-clone-failure
  abort: cannot specify both --noupdate and --updaterev
  [255]

  $ hg clone --all-largefiles a ssh://localhost/a
  abort: --all-largefiles is incompatible with non-local destination ssh://localhost/a
  [255]

Test pulling with --all-largefiles flag.  Also test that the largefiles are
downloaded from 'default' instead of 'default-push' when no source is specified
(issue3584)

  $ rm -Rf a-backup
  $ hg clone -r 1 a a-backup
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 8 changes to 4 files
  updating to branch default
  getting changed largefiles
  2 largefiles updated, 0 removed
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm "${USERCACHE}"/*
  $ cd a-backup
  $ hg pull --all-largefiles --config paths.default-push=bogus/path
  pulling from $TESTTMP/a (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 6 changesets with 16 changes to 8 files
  (run 'hg update' to get a working copy)
  6 largefiles cached

redo pull with --lfrev and check it pulls largefiles for the right revs

  $ hg rollback
  repository tip rolled back to revision 1 (undo pull)
  $ hg pull -v --lfrev 'heads(pulled())+min(pulled())'
  pulling from $TESTTMP/a (glob)
  searching for changes
  all local heads known remotely
  6 changesets found
  adding changesets
  uncompressed size of bundle content:
      1213 (changelog)
      1479 (manifests)
       234  .hglf/large1
       504  .hglf/large3
       512  .hglf/sub/large4
       162  .hglf/sub2/large6
       162  .hglf/sub2/large7
       192  normal1
       397  normal3
       405  sub/normal4
  adding manifests
  adding file changes
  added 6 changesets with 16 changes to 8 files
  calling hook changegroup.lfiles: hgext.largefiles.reposetup.checkrequireslfiles
  (run 'hg update' to get a working copy)
  pulling largefiles for revision 7
  found 971fb41e78fea4f8e0ba5244784239371cb00591 in store
  found 0d6d75887db61b2c7e6c74b5dd8fc6ad50c0cc30 in store
  found bb3151689acb10f0c3125c560d5e63df914bc1af in store
  pulling largefiles for revision 2
  found eb7338044dc27f9bc59b8dd5a246b065ead7a9c4 in store
  0 largefiles cached

lfpull

  $ hg lfpull -r : --config largefiles.usercache=usercache-lfpull
  2 largefiles cached
  $ hg lfpull -v -r 4+2 --config largefiles.usercache=usercache-lfpull
  pulling largefiles for revision 4
  found eb7338044dc27f9bc59b8dd5a246b065ead7a9c4 in store
  found eb7338044dc27f9bc59b8dd5a246b065ead7a9c4 in store
  pulling largefiles for revision 2
  found eb7338044dc27f9bc59b8dd5a246b065ead7a9c4 in store
  0 largefiles cached

  $ ls usercache-lfpull/* | sort
  usercache-lfpull/1deebade43c8c498a3c8daddac0244dc55d1331d
  usercache-lfpull/4669e532d5b2c093a78eca010077e708a071bb64

  $ cd ..

Rebasing between two repositories does not revert largefiles to old
revisions (this was a very bad bug that took a lot of work to fix).

  $ hg clone a d
  updating to branch default
  getting changed largefiles
  3 largefiles updated, 0 removed
  5 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd b
  $ echo large4-modified > sub/large4
  $ echo normal3-modified > normal3
  $ hg commit -m "modify normal file and largefile in repo b"
  Invoking status precommit hook
  M normal3
  M sub/large4
  $ cd ../d
  $ echo large6-modified > sub2/large6
  $ echo normal4-modified > sub/normal4
  $ hg commit -m "modify normal file largefile in repo d"
  Invoking status precommit hook
  M sub/normal4
  M sub2/large6
  $ cd ..
  $ hg clone d e
  updating to branch default
  getting changed largefiles
  3 largefiles updated, 0 removed
  5 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd d

More rebase testing, but also test that the largefiles are downloaded from
'default-push' when no source is specified (issue3584). (The largefile from the
pulled revision is however not downloaded but found in the local cache.)
Largefiles are fetched for the new pulled revision, not for existing revisions,
rebased or not.

  $ [ ! -f .hg/largefiles/e166e74c7303192238d60af5a9c4ce9bef0b7928 ]
  $ hg pull --rebase --all-largefiles --config paths.default-push=bogus/path --config paths.default=../b
  pulling from $TESTTMP/b (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 2 changes to 2 files (+1 heads)
  rebasing 8:f574fb32bb45 "modify normal file largefile in repo d"
  Invoking status precommit hook
  M sub/normal4
  M sub2/large6
  saved backup bundle to $TESTTMP/d/.hg/strip-backup/f574fb32bb45-dd1d9f80-backup.hg (glob)
  0 largefiles cached
  $ [ -f .hg/largefiles/e166e74c7303192238d60af5a9c4ce9bef0b7928 ]
  $ hg log --template '{rev}:{node|short}  {desc|firstline}\n'
  9:598410d3eb9a  modify normal file largefile in repo d
  8:a381d2c8c80e  modify normal file and largefile in repo b
  7:daea875e9014  add/edit more largefiles
  6:4355d653f84f  edit files yet again
  5:9d5af5072dbd  edit files again
  4:74c02385b94c  move files
  3:9e8fbc4bce62  copy files
  2:51a0ae4d5864  remove files
  1:ce8896473775  edit files
  0:30d30fe6a5be  add files
  $ hg log -G --template '{rev}:{node|short}  {desc|firstline}\n'
  @  9:598410d3eb9a  modify normal file largefile in repo d
  |
  o  8:a381d2c8c80e  modify normal file and largefile in repo b
  |
  o  7:daea875e9014  add/edit more largefiles
  |
  o  6:4355d653f84f  edit files yet again
  |
  o  5:9d5af5072dbd  edit files again
  |
  o  4:74c02385b94c  move files
  |
  o  3:9e8fbc4bce62  copy files
  |
  o  2:51a0ae4d5864  remove files
  |
  o  1:ce8896473775  edit files
  |
  o  0:30d30fe6a5be  add files
  
  $ cat normal3
  normal3-modified
  $ cat sub/normal4
  normal4-modified
  $ cat sub/large4
  large4-modified
  $ cat sub2/large6
  large6-modified
  $ cat sub2/large7
  large7
  $ cd ../e
  $ hg pull ../b
  pulling from ../b
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 2 changes to 2 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hg rebase
  rebasing 8:f574fb32bb45 "modify normal file largefile in repo d"
  Invoking status precommit hook
  M sub/normal4
  M sub2/large6
  saved backup bundle to $TESTTMP/e/.hg/strip-backup/f574fb32bb45-dd1d9f80-backup.hg (glob)
  $ hg log --template '{rev}:{node|short}  {desc|firstline}\n'
  9:598410d3eb9a  modify normal file largefile in repo d
  8:a381d2c8c80e  modify normal file and largefile in repo b
  7:daea875e9014  add/edit more largefiles
  6:4355d653f84f  edit files yet again
  5:9d5af5072dbd  edit files again
  4:74c02385b94c  move files
  3:9e8fbc4bce62  copy files
  2:51a0ae4d5864  remove files
  1:ce8896473775  edit files
  0:30d30fe6a5be  add files
  $ cat normal3
  normal3-modified
  $ cat sub/normal4
  normal4-modified
  $ cat sub/large4
  large4-modified
  $ cat sub2/large6
  large6-modified
  $ cat sub2/large7
  large7

Log on largefiles

- same output
  $ hg log --template '{rev}:{node|short}  {desc|firstline}\n' .hglf/sub/large4
  8:a381d2c8c80e  modify normal file and largefile in repo b
  6:4355d653f84f  edit files yet again
  5:9d5af5072dbd  edit files again
  4:74c02385b94c  move files
  $ hg log -G --template '{rev}:{node|short}  {desc|firstline}\n' .hglf/sub/large4
  o  8:a381d2c8c80e  modify normal file and largefile in repo b
  |
  o  6:4355d653f84f  edit files yet again
  |
  o  5:9d5af5072dbd  edit files again
  |
  o  4:74c02385b94c  move files
  |
  $ hg log --template '{rev}:{node|short}  {desc|firstline}\n' sub/large4
  8:a381d2c8c80e  modify normal file and largefile in repo b
  6:4355d653f84f  edit files yet again
  5:9d5af5072dbd  edit files again
  4:74c02385b94c  move files
  $ hg log -G --template '{rev}:{node|short}  {desc|firstline}\n' .hglf/sub/large4
  o  8:a381d2c8c80e  modify normal file and largefile in repo b
  |
  o  6:4355d653f84f  edit files yet again
  |
  o  5:9d5af5072dbd  edit files again
  |
  o  4:74c02385b94c  move files
  |

- .hglf only matches largefiles, without .hglf it matches 9 bco sub/normal
  $ hg log --template '{rev}:{node|short}  {desc|firstline}\n' .hglf/sub
  8:a381d2c8c80e  modify normal file and largefile in repo b
  6:4355d653f84f  edit files yet again
  5:9d5af5072dbd  edit files again
  4:74c02385b94c  move files
  1:ce8896473775  edit files
  0:30d30fe6a5be  add files
  $ hg log -G --template '{rev}:{node|short}  {desc|firstline}\n' .hglf/sub
  o  8:a381d2c8c80e  modify normal file and largefile in repo b
  |
  o  6:4355d653f84f  edit files yet again
  |
  o  5:9d5af5072dbd  edit files again
  |
  o  4:74c02385b94c  move files
  |
  o  1:ce8896473775  edit files
  |
  o  0:30d30fe6a5be  add files
  
  $ hg log --template '{rev}:{node|short}  {desc|firstline}\n' sub
  9:598410d3eb9a  modify normal file largefile in repo d
  8:a381d2c8c80e  modify normal file and largefile in repo b
  6:4355d653f84f  edit files yet again
  5:9d5af5072dbd  edit files again
  4:74c02385b94c  move files
  1:ce8896473775  edit files
  0:30d30fe6a5be  add files
  $ hg log -G --template '{rev}:{node|short}  {desc|firstline}\n' sub
  @  9:598410d3eb9a  modify normal file largefile in repo d
  |
  o  8:a381d2c8c80e  modify normal file and largefile in repo b
  |
  o  6:4355d653f84f  edit files yet again
  |
  o  5:9d5af5072dbd  edit files again
  |
  o  4:74c02385b94c  move files
  |
  o  1:ce8896473775  edit files
  |
  o  0:30d30fe6a5be  add files
  
- globbing gives same result
  $ hg log --template '{rev}:{node|short}  {desc|firstline}\n' 'glob:sub/*'
  9:598410d3eb9a  modify normal file largefile in repo d
  8:a381d2c8c80e  modify normal file and largefile in repo b
  6:4355d653f84f  edit files yet again
  5:9d5af5072dbd  edit files again
  4:74c02385b94c  move files
  1:ce8896473775  edit files
  0:30d30fe6a5be  add files
  $ hg log -G --template '{rev}:{node|short}  {desc|firstline}\n' 'glob:sub/*'
  @  9:598410d3eb9a  modify normal file largefile in repo d
  |
  o  8:a381d2c8c80e  modify normal file and largefile in repo b
  |
  o  6:4355d653f84f  edit files yet again
  |
  o  5:9d5af5072dbd  edit files again
  |
  o  4:74c02385b94c  move files
  |
  o  1:ce8896473775  edit files
  |
  o  0:30d30fe6a5be  add files
  
Rollback on largefiles.

  $ echo large4-modified-again > sub/large4
  $ hg commit -m "Modify large4 again"
  Invoking status precommit hook
  M sub/large4
  $ hg rollback
  repository tip rolled back to revision 9 (undo commit)
  working directory now based on revision 9
  $ hg st
  M sub/large4
  $ hg log --template '{rev}:{node|short}  {desc|firstline}\n'
  9:598410d3eb9a  modify normal file largefile in repo d
  8:a381d2c8c80e  modify normal file and largefile in repo b
  7:daea875e9014  add/edit more largefiles
  6:4355d653f84f  edit files yet again
  5:9d5af5072dbd  edit files again
  4:74c02385b94c  move files
  3:9e8fbc4bce62  copy files
  2:51a0ae4d5864  remove files
  1:ce8896473775  edit files
  0:30d30fe6a5be  add files
  $ cat sub/large4
  large4-modified-again

"update --check" refuses to update with uncommitted changes.
  $ hg update --check 8
  abort: uncommitted changes
  [255]

"update --clean" leaves correct largefiles in working copy, even when there is
.orig files from revert in .hglf.

  $ echo mistake > sub2/large7
  $ hg revert sub2/large7
  $ cat sub2/large7
  large7
  $ cat sub2/large7.orig
  mistake
  $ test ! -f .hglf/sub2/large7.orig

  $ hg -q update --clean -r null
  $ hg update --clean
  getting changed largefiles
  3 largefiles updated, 0 removed
  5 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat normal3
  normal3-modified
  $ cat sub/normal4
  normal4-modified
  $ cat sub/large4
  large4-modified
  $ cat sub2/large6
  large6-modified
  $ cat sub2/large7
  large7
  $ cat sub2/large7.orig
  mistake
  $ test ! -f .hglf/sub2/large7.orig

verify that largefile .orig file no longer is overwritten on every update -C:
  $ hg update --clean
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat sub2/large7.orig
  mistake
  $ rm sub2/large7.orig

Now "update check" is happy.
  $ hg update --check 8
  getting changed largefiles
  1 largefiles updated, 0 removed
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg update --check
  getting changed largefiles
  1 largefiles updated, 0 removed
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

Test removing empty largefiles directories on update
  $ test -d sub2 && echo "sub2 exists"
  sub2 exists
  $ hg update -q null
  $ test -d sub2 && echo "error: sub2 should not exist anymore"
  [1]
  $ hg update -q

Test hg remove removes empty largefiles directories
  $ test -d sub2 && echo "sub2 exists"
  sub2 exists
  $ hg remove sub2/*
  $ test -d sub2 && echo "error: sub2 should not exist anymore"
  [1]
  $ hg revert sub2/large6 sub2/large7

"revert" works on largefiles (and normal files too).
  $ echo hack3 >> normal3
  $ echo hack4 >> sub/normal4
  $ echo hack4 >> sub/large4
  $ rm sub2/large6
  $ hg revert sub2/large6
  $ hg rm sub2/large6
  $ echo new >> sub2/large8
  $ hg add --large sub2/large8
# XXX we don't really want to report that we're reverting the standin;
# that's just an implementation detail. But I don't see an obvious fix. ;-(
  $ hg revert sub
  reverting .hglf/sub/large4 (glob)
  reverting sub/normal4 (glob)
  $ hg status
  M normal3
  A sub2/large8
  R sub2/large6
  ? sub/large4.orig
  ? sub/normal4.orig
  $ cat sub/normal4
  normal4-modified
  $ cat sub/large4
  large4-modified
  $ hg revert -a --no-backup
  undeleting .hglf/sub2/large6 (glob)
  forgetting .hglf/sub2/large8 (glob)
  reverting normal3
  $ hg status
  ? sub/large4.orig
  ? sub/normal4.orig
  ? sub2/large8
  $ cat normal3
  normal3-modified
  $ cat sub2/large6
  large6-modified
  $ rm sub/*.orig sub2/large8

revert some files to an older revision
  $ hg revert --no-backup -r 8 sub2
  reverting .hglf/sub2/large6 (glob)
  $ cat sub2/large6
  large6
  $ hg revert --no-backup -C -r '.^' sub2
  $ hg revert --no-backup sub2
  reverting .hglf/sub2/large6 (glob)
  $ hg status

"verify --large" actually verifies largefiles

- Where Do We Come From? What Are We? Where Are We Going?
  $ pwd
  $TESTTMP/e
  $ hg paths
  default = $TESTTMP/d (glob)

  $ hg verify --large
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  10 files, 10 changesets, 28 total revisions
  searching 1 changesets for largefiles
  verified existence of 3 revisions of 3 largefiles

- introduce missing blob in local store repo and make sure that this is caught:
  $ mv $TESTTMP/d/.hg/largefiles/e166e74c7303192238d60af5a9c4ce9bef0b7928 .
  $ hg verify --large
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  10 files, 10 changesets, 28 total revisions
  searching 1 changesets for largefiles
  changeset 9:598410d3eb9a: sub/large4 references missing $TESTTMP/d/.hg/largefiles/e166e74c7303192238d60af5a9c4ce9bef0b7928 (glob)
  verified existence of 3 revisions of 3 largefiles
  [1]

- introduce corruption and make sure that it is caught when checking content:
  $ echo '5 cents' > $TESTTMP/d/.hg/largefiles/e166e74c7303192238d60af5a9c4ce9bef0b7928
  $ hg verify -q --large --lfc
  changeset 9:598410d3eb9a: sub/large4 references corrupted $TESTTMP/d/.hg/largefiles/e166e74c7303192238d60af5a9c4ce9bef0b7928 (glob)
  [1]

- cleanup
  $ mv e166e74c7303192238d60af5a9c4ce9bef0b7928 $TESTTMP/d/.hg/largefiles/

- verifying all revisions will fail because we didn't clone all largefiles to d:
  $ echo 'T-shirt' > $TESTTMP/d/.hg/largefiles/eb7338044dc27f9bc59b8dd5a246b065ead7a9c4
  $ hg verify -q --lfa --lfc
  changeset 0:30d30fe6a5be: large1 references missing $TESTTMP/d/.hg/largefiles/4669e532d5b2c093a78eca010077e708a071bb64 (glob)
  changeset 0:30d30fe6a5be: sub/large2 references missing $TESTTMP/d/.hg/largefiles/1deebade43c8c498a3c8daddac0244dc55d1331d (glob)
  changeset 1:ce8896473775: large1 references missing $TESTTMP/d/.hg/largefiles/5f78770c0e77ba4287ad6ef3071c9bf9c379742f (glob)
  changeset 1:ce8896473775: sub/large2 references corrupted $TESTTMP/d/.hg/largefiles/eb7338044dc27f9bc59b8dd5a246b065ead7a9c4 (glob)
  changeset 3:9e8fbc4bce62: large1 references corrupted $TESTTMP/d/.hg/largefiles/eb7338044dc27f9bc59b8dd5a246b065ead7a9c4 (glob)
  changeset 4:74c02385b94c: large3 references corrupted $TESTTMP/d/.hg/largefiles/eb7338044dc27f9bc59b8dd5a246b065ead7a9c4 (glob)
  changeset 4:74c02385b94c: sub/large4 references corrupted $TESTTMP/d/.hg/largefiles/eb7338044dc27f9bc59b8dd5a246b065ead7a9c4 (glob)
  changeset 5:9d5af5072dbd: large3 references missing $TESTTMP/d/.hg/largefiles/baaf12afde9d8d67f25dab6dced0d2bf77dba47c (glob)
  changeset 5:9d5af5072dbd: sub/large4 references missing $TESTTMP/d/.hg/largefiles/aeb2210d19f02886dde00dac279729a48471e2f9 (glob)
  changeset 6:4355d653f84f: large3 references missing $TESTTMP/d/.hg/largefiles/7838695e10da2bb75ac1156565f40a2595fa2fa0 (glob)
  [1]

- cleanup
  $ rm $TESTTMP/d/.hg/largefiles/eb7338044dc27f9bc59b8dd5a246b065ead7a9c4
  $ rm -f .hglf/sub/*.orig

Update to revision with missing largefile - and make sure it really is missing

  $ rm ${USERCACHE}/7838695e10da2bb75ac1156565f40a2595fa2fa0
  $ hg up -r 6
  getting changed largefiles
  large3: largefile 7838695e10da2bb75ac1156565f40a2595fa2fa0 not available from file:/*/$TESTTMP/d (glob)
  1 largefiles updated, 2 removed
  4 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ rm normal3
  $ echo >> sub/normal4
  $ hg ci -m 'commit with missing files'
  Invoking status precommit hook
  M sub/normal4
  ! large3
  ! normal3
  created new head
  $ hg st
  ! large3
  ! normal3
  $ hg up -r.
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg st
  ! large3
  ! normal3
  $ hg up -Cr.
  getting changed largefiles
  large3: largefile 7838695e10da2bb75ac1156565f40a2595fa2fa0 not available from file:/*/$TESTTMP/d (glob)
  0 largefiles updated, 0 removed
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg st
  ! large3
  $ hg rollback
  repository tip rolled back to revision 9 (undo commit)
  working directory now based on revision 6

Merge with revision with missing largefile - and make sure it tries to fetch it.

  $ hg up -Cqr null
  $ echo f > f
  $ hg ci -Am branch
  adding f
  Invoking status precommit hook
  A f
  created new head
  $ hg merge -r 6
  getting changed largefiles
  large3: largefile 7838695e10da2bb75ac1156565f40a2595fa2fa0 not available from file:/*/$TESTTMP/d (glob)
  1 largefiles updated, 0 removed
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ hg rollback -q
  $ hg up -Cq

Pulling 0 revisions with --all-largefiles should not fetch for all revisions

  $ hg pull --all-largefiles
  pulling from $TESTTMP/d (glob)
  searching for changes
  no changes found

Merging does not revert to old versions of largefiles and also check
that merging after having pulled from a non-default remote works
correctly.

  $ cd ..
  $ hg clone -r 7 e temp
  adding changesets
  adding manifests
  adding file changes
  added 8 changesets with 24 changes to 10 files
  updating to branch default
  getting changed largefiles
  3 largefiles updated, 0 removed
  5 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg clone temp f
  updating to branch default
  getting changed largefiles
  3 largefiles updated, 0 removed
  5 files updated, 0 files merged, 0 files removed, 0 files unresolved
# Delete the largefiles in the largefiles system cache so that we have an
# opportunity to test that caching after a pull works.
  $ rm "${USERCACHE}"/*
  $ cd f
  $ echo "large4-merge-test" > sub/large4
  $ hg commit -m "Modify large4 to test merge"
  Invoking status precommit hook
  M sub/large4
# Test --cache-largefiles flag
  $ hg pull --lfrev 'heads(pulled())' ../e
  pulling from ../e
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 4 changes to 4 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)
  2 largefiles cached
  $ hg merge
  largefile sub/large4 has a merge conflict
  ancestor was 971fb41e78fea4f8e0ba5244784239371cb00591
  keep (l)ocal d846f26643bfa8ec210be40cc93cc6b7ff1128ea or
  take (o)ther e166e74c7303192238d60af5a9c4ce9bef0b7928? l
  getting changed largefiles
  1 largefiles updated, 0 removed
  3 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg commit -m "Merge repos e and f"
  Invoking status precommit hook
  M normal3
  M sub/normal4
  M sub2/large6
  $ cat normal3
  normal3-modified
  $ cat sub/normal4
  normal4-modified
  $ cat sub/large4
  large4-merge-test
  $ cat sub2/large6
  large6-modified
  $ cat sub2/large7
  large7

Test status after merging with a branch that introduces a new largefile:

  $ echo large > large
  $ hg add --large large
  $ hg commit -m 'add largefile'
  Invoking status precommit hook
  A large
  $ hg update -q ".^"
  $ echo change >> normal3
  $ hg commit -m 'some change'
  Invoking status precommit hook
  M normal3
  created new head
  $ hg merge
  getting changed largefiles
  1 largefiles updated, 0 removed
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg status
  M large

- make sure update of merge with removed largefiles fails as expected
  $ hg rm sub2/large6
  $ hg up -r.
  abort: outstanding uncommitted merge
  [255]

- revert should be able to revert files introduced in a pending merge
  $ hg revert --all -r .
  removing .hglf/large (glob)
  undeleting .hglf/sub2/large6 (glob)

Test that a normal file and a largefile with the same name and path cannot
coexist.

  $ rm sub2/large7
  $ echo "largeasnormal" > sub2/large7
  $ hg add sub2/large7
  sub2/large7 already a largefile (glob)

Test that transplanting a largefile change works correctly.

  $ cd ..
  $ hg clone -r 8 d g
  adding changesets
  adding manifests
  adding file changes
  added 9 changesets with 26 changes to 10 files
  updating to branch default
  getting changed largefiles
  3 largefiles updated, 0 removed
  5 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd g
  $ hg transplant -s ../d 598410d3eb9a
  searching for changes
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 2 changes to 2 files
  $ hg log --template '{rev}:{node|short}  {desc|firstline}\n'
  9:598410d3eb9a  modify normal file largefile in repo d
  8:a381d2c8c80e  modify normal file and largefile in repo b
  7:daea875e9014  add/edit more largefiles
  6:4355d653f84f  edit files yet again
  5:9d5af5072dbd  edit files again
  4:74c02385b94c  move files
  3:9e8fbc4bce62  copy files
  2:51a0ae4d5864  remove files
  1:ce8896473775  edit files
  0:30d30fe6a5be  add files
  $ cat normal3
  normal3-modified
  $ cat sub/normal4
  normal4-modified
  $ cat sub/large4
  large4-modified
  $ cat sub2/large6
  large6-modified
  $ cat sub2/large7
  large7

Cat a largefile
  $ hg cat normal3
  normal3-modified
  $ hg cat sub/large4
  large4-modified
  $ rm "${USERCACHE}"/*
  $ hg cat -r a381d2c8c80e -o cat.out sub/large4
  $ cat cat.out
  large4-modified
  $ rm cat.out
  $ hg cat -r a381d2c8c80e normal3
  normal3-modified
  $ hg cat -r '.^' normal3
  normal3-modified
  $ hg cat -r '.^' sub/large4 doesntexist
  large4-modified
  doesntexist: no such file in rev a381d2c8c80e
  $ hg --cwd sub cat -r '.^' large4
  large4-modified
  $ hg --cwd sub cat -r '.^' ../normal3
  normal3-modified
Cat a standin
  $ hg cat .hglf/sub/large4
  e166e74c7303192238d60af5a9c4ce9bef0b7928
  $ hg cat .hglf/normal3
  .hglf/normal3: no such file in rev 598410d3eb9a (glob)
  [1]

Test that renaming a largefile results in correct output for status

  $ hg rename sub/large4 large4-renamed
  $ hg commit -m "test rename output"
  Invoking status precommit hook
  A large4-renamed
  R sub/large4
  $ cat large4-renamed
  large4-modified
  $ cd sub2
  $ hg rename large6 large6-renamed
  $ hg st
  A sub2/large6-renamed
  R sub2/large6
  $ cd ..

Test --normal flag

  $ dd if=/dev/zero bs=2k count=11k > new-largefile 2> /dev/null
  $ hg add --normal --large new-largefile
  abort: --normal cannot be used with --large
  [255]
  $ hg add --normal new-largefile
  new-largefile: up to 69 MB of RAM may be required to manage this file
  (use 'hg revert new-largefile' to cancel the pending addition)
  $ cd ..



