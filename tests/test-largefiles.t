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
files, testing that status correctly shows largefiles and that summary output
is correct.

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
  largefiles: No remote repo

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

Test exit codes for remove warning cases (modified and still exiting)

  $ hg remove -A large1
  not removing large1: file still exists (use forget to undo)
  [1]
  $ echo 'modified' > large1
  $ hg remove large1
  not removing large1: file is modified (use forget to undo)
  [1]
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

Test copies and moves from a directory other than root (issue3516)

  $ cd ..
  $ hg init lf_cpmv
  $ cd lf_cpmv
  $ mkdir dira
  $ mkdir dira/dirb
  $ touch dira/dirb/largefile
  $ hg add --large dira/dirb/largefile
  $ hg commit -m "added"
  Invoking status precommit hook
  A dira/dirb/largefile
  $ cd dira
  $ hg cp dirb/largefile foo/largefile
  $ hg ci -m "deep copy"
  Invoking status precommit hook
  A dira/foo/largefile
  $ find . | sort
  .
  ./dirb
  ./dirb/largefile
  ./foo
  ./foo/largefile
  $ hg mv foo/largefile baz/largefile
  $ hg ci -m "moved"
  Invoking status precommit hook
  A dira/baz/largefile
  R dira/foo/largefile
  $ find . | sort
  .
  ./baz
  ./baz/largefile
  ./dirb
  ./dirb/largefile
  ./foo
  $ cd ../../a

#if hgweb
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

Test addremove: verify that files that should be added as largfiles are added as
such and that already-existing largfiles are not added as normal files by
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
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  getting changed largefiles
  1 largefiles updated, 0 removed
  $ rm normal3
  $ rm sub/large4
  $ echo "testing addremove with patterns" > testaddremove.dat
  $ echo "normaladdremove" > normaladdremove
  $ cd ..
  $ hg -R a addremove
  removing sub/large4
  adding a/testaddremove.dat as a largefile (glob)
  removing normal3
  adding normaladdremove
  $ cd a

Test 3364
  $ hg clone . ../addrm
  updating to branch default
  5 files updated, 0 files merged, 0 files removed, 0 files unresolved
  getting changed largefiles
  3 largefiles updated, 0 removed
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
  getting changed largefiles
  0 largefiles updated, 0 removed
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
  
  searching for changes
  largefiles to upload:
  large
  foo
  
  $ cd ../a

Clone a largefiles repo.

  $ hg clone . ../b
  updating to branch default
  5 files updated, 0 files merged, 0 files removed, 0 files unresolved
  getting changed largefiles
  3 largefiles updated, 0 removed
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
  $ cat sub/normal4
  normal44
  $ cat sub/large4
  large44
  $ cat sub2/large6
  large6
  $ cat sub2/large7
  large7
  $ cd ..
  $ hg clone a -r 3 c
  adding changesets
  adding manifests
  adding file changes
  added 4 changesets with 10 changes to 4 files
  updating to branch default
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  getting changed largefiles
  2 largefiles updated, 0 removed
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
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  getting changed largefiles
  1 largefiles updated, 0 removed
  $ cat large1
  large11
  $ cat sub/large2
  large22
  $ cd ..

Test cloning with --all-largefiles flag

  $ rm "${USERCACHE}"/*
  $ hg clone --all-largefiles a a-backup
  updating to branch default
  5 files updated, 0 files merged, 0 files removed, 0 files unresolved
  getting changed largefiles
  3 largefiles updated, 0 removed
  8 additional largefiles cached

  $ hg clone --all-largefiles a ssh://localhost/a
  abort: --all-largefiles is incompatible with non-local destination ssh://localhost/a
  [255]

Test pulling with --all-largefiles flag

  $ rm -Rf a-backup
  $ hg clone -r 1 a a-backup
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 8 changes to 4 files
  updating to branch default
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  getting changed largefiles
  2 largefiles updated, 0 removed
  $ rm "${USERCACHE}"/*
  $ cd a-backup
  $ hg pull --all-largefiles
  pulling from $TESTTMP/a (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 6 changesets with 16 changes to 8 files
  (run 'hg update' to get a working copy)
  caching new largefiles
  3 largefiles cached
  3 additional largefiles cached
  $ cd ..

Rebasing between two repositories does not revert largefiles to old
revisions (this was a very bad bug that took a lot of work to fix).

  $ hg clone a d
  updating to branch default
  5 files updated, 0 files merged, 0 files removed, 0 files unresolved
  getting changed largefiles
  3 largefiles updated, 0 removed
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
  5 files updated, 0 files merged, 0 files removed, 0 files unresolved
  getting changed largefiles
  3 largefiles updated, 0 removed
  $ cd d
  $ hg pull --rebase ../b
  pulling from ../b
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 2 changes to 2 files (+1 heads)
  Invoking status precommit hook
  M sub/normal4
  M sub2/large6
  saved backup bundle to $TESTTMP/d/.hg/strip-backup/f574fb32bb45-backup.hg (glob)
  nothing to rebase
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
  $ cd ../e
  $ hg pull ../b
  pulling from ../b
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 2 changes to 2 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)
  caching new largefiles
  0 largefiles cached
  $ hg rebase
  Invoking status precommit hook
  M sub/normal4
  M sub2/large6
  saved backup bundle to $TESTTMP/e/.hg/strip-backup/f574fb32bb45-backup.hg (glob)
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
  abort: uncommitted local changes
  [255]

"update --clean" leaves correct largefiles in working copy.

  $ hg update --clean
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  getting changed largefiles
  1 largefiles updated, 0 removed
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

Now "update check" is happy.
  $ hg update --check 8
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  getting changed largefiles
  1 largefiles updated, 0 removed
  $ hg update --check
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  getting changed largefiles
  1 largefiles updated, 0 removed

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
  reverting .hglf/sub2/large6 (glob)
  $ hg revert --no-backup sub2
  reverting .hglf/sub2/large6 (glob)
  $ hg status

"verify --large" actually verifies largefiles

  $ hg verify --large
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  10 files, 10 changesets, 28 total revisions
  searching 1 changesets for largefiles
  verified existence of 3 revisions of 3 largefiles

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
  5 files updated, 0 files merged, 0 files removed, 0 files unresolved
  getting changed largefiles
  3 largefiles updated, 0 removed
  $ hg clone temp f
  updating to branch default
  5 files updated, 0 files merged, 0 files removed, 0 files unresolved
  getting changed largefiles
  3 largefiles updated, 0 removed
# Delete the largefiles in the largefiles system cache so that we have an
# opportunity to test that caching after a pull works.
  $ rm "${USERCACHE}"/*
  $ cd f
  $ echo "large4-merge-test" > sub/large4
  $ hg commit -m "Modify large4 to test merge"
  Invoking status precommit hook
  M sub/large4
  $ hg pull ../e
  pulling from ../e
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 4 changes to 4 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)
  caching new largefiles
  2 largefiles cached
  $ hg merge
  merging sub/large4
  largefile sub/large4 has a merge conflict
  keep (l)ocal or take (o)ther? l
  3 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  getting changed largefiles
  1 largefiles updated, 0 removed
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
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  getting changed largefiles
  1 largefiles updated, 0 removed
  $ hg status
  M large

Test that a normal file and a largefile with the same name and path cannot
coexist.

  $ rm sub2/large7
  $ echo "largeasnormal" > sub2/large7
  $ hg add sub2/large7
  sub2/large7 already a largefile

Test that transplanting a largefile change works correctly.

  $ cd ..
  $ hg clone -r 8 d g
  adding changesets
  adding manifests
  adding file changes
  added 9 changesets with 26 changes to 10 files
  updating to branch default
  5 files updated, 0 files merged, 0 files removed, 0 files unresolved
  getting changed largefiles
  3 largefiles updated, 0 removed
  $ cd g
  $ hg transplant -s ../d 598410d3eb9a
  searching for changes
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 2 changes to 2 files
  getting changed largefiles
  1 largefiles updated, 0 removed
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
  $ hg cat -r '.^' sub/large4
  large4-modified

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

#if serve
vanilla clients not locked out from largefiles servers on vanilla repos
  $ mkdir r1
  $ cd r1
  $ hg init
  $ echo c1 > f1
  $ hg add f1
  $ hg commit -m "m1"
  Invoking status precommit hook
  A f1
  $ cd ..
  $ hg serve -R r1 -d -p $HGPORT --pid-file hg.pid
  $ cat hg.pid >> $DAEMON_PIDS
  $ hg --config extensions.largefiles=! clone http://localhost:$HGPORT r2
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

largefiles clients still work with vanilla servers
  $ hg --config extensions.largefiles=! serve -R r1 -d -p $HGPORT1 --pid-file hg.pid
  $ cat hg.pid >> $DAEMON_PIDS
  $ hg clone http://localhost:$HGPORT1 r3
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
#endif


vanilla clients locked out from largefiles http repos
  $ mkdir r4
  $ cd r4
  $ hg init
  $ echo c1 > f1
  $ hg add --large f1
  $ hg commit -m "m1"
  Invoking status precommit hook
  A f1
  $ cd ..

largefiles can be pushed locally (issue3583)
  $ hg init dest
  $ cd r4
  $ hg outgoing ../dest
  comparing with ../dest
  searching for changes
  changeset:   0:639881c12b4c
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     m1
  
  $ hg push ../dest
  pushing to ../dest
  searching for changes
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files

exit code with nothing outgoing (issue3611)
  $ hg outgoing ../dest
  comparing with ../dest
  searching for changes
  no changes found
  [1]
  $ cd ..

#if serve
  $ hg serve -R r4 -d -p $HGPORT2 --pid-file hg.pid
  $ cat hg.pid >> $DAEMON_PIDS
  $ hg --config extensions.largefiles=! clone http://localhost:$HGPORT2 r5
  abort: remote error:
  
  This repository uses the largefiles extension.
  
  Please enable it in your Mercurial config file.
  [255]

used all HGPORTs, kill all daemons
  $ "$TESTDIR/killdaemons.py" $DAEMON_PIDS
#endif

vanilla clients locked out from largefiles ssh repos
  $ hg --config extensions.largefiles=! clone -e "python \"$TESTDIR/dummyssh\"" ssh://user@dummy/r4 r5
  abort: remote error:
  
  This repository uses the largefiles extension.
  
  Please enable it in your Mercurial config file.
  [255]

#if serve

largefiles clients refuse to push largefiles repos to vanilla servers
  $ mkdir r6
  $ cd r6
  $ hg init
  $ echo c1 > f1
  $ hg add f1
  $ hg commit -m "m1"
  Invoking status precommit hook
  A f1
  $ cat >> .hg/hgrc <<!
  > [web]
  > push_ssl = false
  > allow_push = *
  > !
  $ cd ..
  $ hg clone r6 r7
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd r7
  $ echo c2 > f2
  $ hg add --large f2
  $ hg commit -m "m2"
  Invoking status precommit hook
  A f2
  $ hg --config extensions.largefiles=! -R ../r6 serve -d -p $HGPORT --pid-file ../hg.pid
  $ cat ../hg.pid >> $DAEMON_PIDS
  $ hg push http://localhost:$HGPORT
  pushing to http://localhost:$HGPORT/
  searching for changes
  abort: http://localhost:$HGPORT/ does not appear to be a largefile store
  [255]
  $ cd ..

putlfile errors are shown (issue3123)
Corrupt the cached largefile in r7 and in the usercache (required for testing on vfat)
  $ echo corruption > "$TESTTMP/r7/.hg/largefiles/4cdac4d8b084d0b599525cf732437fb337d422a8"
  $ echo corruption > "$USERCACHE/4cdac4d8b084d0b599525cf732437fb337d422a8"
  $ hg init empty
  $ hg serve -R empty -d -p $HGPORT1 --pid-file hg.pid \
  >   --config 'web.allow_push=*' --config web.push_ssl=False
  $ cat hg.pid >> $DAEMON_PIDS
  $ hg push -R r7 http://localhost:$HGPORT1
  pushing to http://localhost:$HGPORT1/
  searching for changes
  remote: largefiles: failed to put 4cdac4d8b084d0b599525cf732437fb337d422a8 into store: largefile contents do not match hash
  abort: remotestore: could not put $TESTTMP/r7/.hg/largefiles/4cdac4d8b084d0b599525cf732437fb337d422a8 to remote store http://localhost:$HGPORT1/ (glob)
  [255]
  $ rm -rf empty

Push a largefiles repository to a served empty repository
  $ hg init r8
  $ echo c3 > r8/f1
  $ hg add --large r8/f1 -R r8
  $ hg commit -m "m1" -R r8
  Invoking status precommit hook
  A f1
  $ hg init empty
  $ hg serve -R empty -d -p $HGPORT2 --pid-file hg.pid \
  >   --config 'web.allow_push=*' --config web.push_ssl=False
  $ cat hg.pid >> $DAEMON_PIDS
  $ rm "${USERCACHE}"/*
  $ hg push -R r8 http://localhost:$HGPORT2
  pushing to http://localhost:$HGPORT2/
  searching for changes
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files
  $ rm -rf empty

used all HGPORTs, kill all daemons
  $ "$TESTDIR/killdaemons.py" $DAEMON_PIDS

#endif


#if unix-permissions

Clone a local repository owned by another user
We have to simulate that here by setting $HOME and removing write permissions
  $ ORIGHOME="$HOME"
  $ mkdir alice
  $ HOME="`pwd`/alice"
  $ cd alice
  $ hg init pubrepo
  $ cd pubrepo
  $ dd if=/dev/zero bs=1k count=11k > a-large-file 2> /dev/null
  $ hg add --large a-large-file
  $ hg commit -m "Add a large file"
  Invoking status precommit hook
  A a-large-file
  $ cd ..
  $ chmod -R a-w pubrepo
  $ cd ..
  $ mkdir bob
  $ HOME="`pwd`/bob"
  $ cd bob
  $ hg clone --pull ../alice/pubrepo pubrepo
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  getting changed largefiles
  1 largefiles updated, 0 removed
  $ cd ..
  $ chmod -R u+w alice/pubrepo
  $ HOME="$ORIGHOME"

#endif

#if symlink

Symlink to a large largefile should behave the same as a symlink to a normal file
  $ hg init largesymlink
  $ cd largesymlink
  $ dd if=/dev/zero bs=1k count=10k of=largefile 2>/dev/null
  $ hg add --large largefile
  $ hg commit -m "commit a large file"
  Invoking status precommit hook
  A largefile
  $ ln -s largefile largelink
  $ hg add largelink
  $ hg commit -m "commit a large symlink"
  Invoking status precommit hook
  A largelink
  $ rm -f largelink
  $ hg up >/dev/null
  $ test -f largelink
  [1]
  $ test -L largelink
  [1]
  $ rm -f largelink # make next part of the test independent of the previous
  $ hg up -C >/dev/null
  $ test -f largelink
  $ test -L largelink
  $ cd ..

#endif

test for pattern matching on 'hg status':
to boost performance, largefiles checks whether specified patterns are
related to largefiles in working directory (NOT to STANDIN) or not.

  $ hg init statusmatch
  $ cd statusmatch

  $ mkdir -p a/b/c/d
  $ echo normal > a/b/c/d/e.normal.txt
  $ hg add a/b/c/d/e.normal.txt
  $ echo large > a/b/c/d/e.large.txt
  $ hg add --large a/b/c/d/e.large.txt
  $ mkdir -p a/b/c/x
  $ echo normal > a/b/c/x/y.normal.txt
  $ hg add a/b/c/x/y.normal.txt
  $ hg commit -m 'add files'
  Invoking status precommit hook
  A a/b/c/d/e.large.txt
  A a/b/c/d/e.normal.txt
  A a/b/c/x/y.normal.txt

(1) no pattern: no performance boost
  $ hg status -A
  C a/b/c/d/e.large.txt
  C a/b/c/d/e.normal.txt
  C a/b/c/x/y.normal.txt

(2) pattern not related to largefiles: performance boost
  $ hg status -A a/b/c/x
  C a/b/c/x/y.normal.txt

(3) pattern related to largefiles: no performance boost
  $ hg status -A a/b/c/d
  C a/b/c/d/e.large.txt
  C a/b/c/d/e.normal.txt

(4) pattern related to STANDIN (not to largefiles): performance boost
  $ hg status -A .hglf/a
  C .hglf/a/b/c/d/e.large.txt

(5) mixed case: no performance boost
  $ hg status -A a/b/c/x a/b/c/d
  C a/b/c/d/e.large.txt
  C a/b/c/d/e.normal.txt
  C a/b/c/x/y.normal.txt

verify that largefiles doesn't break filesets

  $ hg log --rev . --exclude "set:binary()"
  changeset:   0:41bd42f10efa
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add files
  
verify that large files in subrepos handled properly
  $ hg init subrepo
  $ echo "subrepo = subrepo" > .hgsub
  $ hg add .hgsub
  $ hg ci -m "add subrepo"
  Invoking status precommit hook
  A .hgsub
  ? .hgsubstate
  $ echo "rev 1" > subrepo/large.txt
  $ hg -R subrepo add --large subrepo/large.txt
  $ hg sum
  parent: 1:8ee150ea2e9c tip
   add subrepo
  branch: default
  commit: 1 subrepos
  update: (current)
  $ hg st
  $ hg st -S
  A subrepo/large.txt
  $ hg ci -S -m "commit top repo"
  committing subrepository subrepo
  Invoking status precommit hook
  A large.txt
  Invoking status precommit hook
  M .hgsubstate
# No differences
  $ hg st -S
  $ hg sum
  parent: 2:ce4cd0c527a6 tip
   commit top repo
  branch: default
  commit: (clean)
  update: (current)
  $ echo "rev 2" > subrepo/large.txt
  $ hg st -S
  M subrepo/large.txt
  $ hg sum
  parent: 2:ce4cd0c527a6 tip
   commit top repo
  branch: default
  commit: 1 subrepos
  update: (current)
  $ hg ci -m "this commit should fail without -S"
  abort: uncommitted changes in subrepo subrepo
  (use --subrepos for recursive commit)
  [255]

Add a normal file to the subrepo, then test archiving

  $ echo 'normal file' > subrepo/normal.txt
  $ hg -R subrepo add subrepo/normal.txt

Lock in subrepo, otherwise the change isn't archived

  $ hg ci -S -m "add normal file to top level"
  committing subrepository subrepo
  Invoking status precommit hook
  M large.txt
  A normal.txt
  Invoking status precommit hook
  M .hgsubstate
  $ hg archive -S lf_subrepo_archive
  $ find lf_subrepo_archive | sort
  lf_subrepo_archive
  lf_subrepo_archive/.hg_archival.txt
  lf_subrepo_archive/.hgsub
  lf_subrepo_archive/.hgsubstate
  lf_subrepo_archive/a
  lf_subrepo_archive/a/b
  lf_subrepo_archive/a/b/c
  lf_subrepo_archive/a/b/c/d
  lf_subrepo_archive/a/b/c/d/e.large.txt
  lf_subrepo_archive/a/b/c/d/e.normal.txt
  lf_subrepo_archive/a/b/c/x
  lf_subrepo_archive/a/b/c/x/y.normal.txt
  lf_subrepo_archive/subrepo
  lf_subrepo_archive/subrepo/large.txt
  lf_subrepo_archive/subrepo/normal.txt

  $ cd ..
