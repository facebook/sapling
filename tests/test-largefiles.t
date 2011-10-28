  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > largefiles=
  > purge=
  > rebase=
  > transplant=
  > [largefiles]
  > minsize=2
  > patterns=glob:**.dat
  > EOF

Create the repo with a couple of revisions of both large and normal
files, testing that status correctly shows largefiles.

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
  $ echo normal11 > normal1
  $ echo normal22 > sub/normal2
  $ echo large11 > large1
  $ echo large22 > sub/large2
  $ hg st
  M large1
  M normal1
  M sub/large2
  M sub/normal2
  $ hg commit -m "edit files"

Commit preserved largefile contents.

  $ cat normal1
  normal11
  $ cat large1
  large11
  $ cat sub/normal2
  normal22
  $ cat sub/large2
  large22

Remove both largefiles and normal files.
 
  $ hg remove normal1 large1
  $ hg commit -m "remove files"
  $ ls
  sub

Copy both largefiles and normal files.

  $ hg cp sub/normal2 normal1
  $ hg cp sub/large2 large1
  $ hg commit -m "copy files"
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
  $ cat normal3
  normal22
  $ cat large3
  large22
  $ cat sub/normal4
  normal22
  $ cat sub/large4
  large22

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
  adding sub2/large6 as a largefile
  adding sub2/large7 as a largefile
  $ hg st
  M large3
  A large5
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
  $ hg st
  A sub2/large6
  A sub2/large7
  R large3
  ? large5
  ? medium
  ? notlarge
  ? ratherlarge
  ? reallylarge
  ? test.dat
  $ hg commit -m "add/edit more largefiles"
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

Clone a largefiles repo.

  $ cd ..
  $ hg clone a b
  updating to branch default
  5 files updated, 0 files merged, 0 files removed, 0 files unresolved
  getting changed largefiles
  3 largefiles updated, 0 removed
  $ cd b
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

Rebasing between two repositories does not revert largefiles to old
revisions (this was a very bad bug that took a lot of work to fix).

  $ cd ..
  $ hg clone a d
  updating to branch default
  5 files updated, 0 files merged, 0 files removed, 0 files unresolved
  getting changed largefiles
  3 largefiles updated, 0 removed
  $ cd b
  $ echo large4-modified > sub/large4
  $ echo normal3-modified > normal3
  $ hg commit -m "modify normal file and largefile in repo b"
  $ cd ../d
  $ echo large6-modified > sub2/large6
  $ echo normal4-modified > sub/normal4
  $ hg commit -m "modify normal file largefile in repo d"
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
  getting changed largefiles
  1 largefiles updated, 0 removed
  saved backup bundle to $TESTTMP/d/.hg/strip-backup/f574fb32bb45-backup.hg
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
  $ hg rebase
  getting changed largefiles
  1 largefiles updated, 0 removed
  saved backup bundle to $TESTTMP/e/.hg/strip-backup/f574fb32bb45-backup.hg
  $ hg log
  changeset:   9:598410d3eb9a
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     modify normal file largefile in repo d
  
  changeset:   8:a381d2c8c80e
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     modify normal file and largefile in repo b
  
  changeset:   7:daea875e9014
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add/edit more largefiles
  
  changeset:   6:4355d653f84f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     edit files yet again
  
  changeset:   5:9d5af5072dbd
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     edit files again
  
  changeset:   4:74c02385b94c
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     move files
  
  changeset:   3:9e8fbc4bce62
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     copy files
  
  changeset:   2:51a0ae4d5864
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     remove files
  
  changeset:   1:ce8896473775
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     edit files
  
  changeset:   0:30d30fe6a5be
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add files
  
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
  $ hg rollback
  repository tip rolled back to revision 9 (undo commit)
  working directory now based on revision 9
  $ hg st
  M sub/large4
  $ hg log
  changeset:   9:598410d3eb9a
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     modify normal file largefile in repo d
  
  changeset:   8:a381d2c8c80e
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     modify normal file and largefile in repo b
  
  changeset:   7:daea875e9014
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add/edit more largefiles
  
  changeset:   6:4355d653f84f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     edit files yet again
  
  changeset:   5:9d5af5072dbd
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     edit files again
  
  changeset:   4:74c02385b94c
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     move files
  
  changeset:   3:9e8fbc4bce62
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     copy files
  
  changeset:   2:51a0ae4d5864
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     remove files
  
  changeset:   1:ce8896473775
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     edit files
  
  changeset:   0:30d30fe6a5be
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add files
  
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

"revert" works on largefiles (and normal files too).
  $ echo hack3 >> normal3
  $ echo hack4 >> sub/normal4
  $ echo hack4 >> sub/large4
  $ hg rm sub2/large6
  $ echo new >> sub2/large8
  $ hg add --large sub2/large8
# XXX we don't really want to report that we're reverting the standin;
# that's just an implementation detail. But I don't see an obvious fix. ;-(
  $ hg revert sub
  reverting .hglf/sub/large4
  reverting sub/normal4
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
  undeleting .hglf/sub2/large6
  forgetting .hglf/sub2/large8
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
  reverting .hglf/sub2/large6
  $ cat sub2/large6
  large6
  $ hg revert --no-backup sub2
  reverting .hglf/sub2/large6
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

Merging does not revert to old versions of largefiles (this has also
been very problematic).

  $ cd ..
  $ hg clone -r 7 e f
  adding changesets
  adding manifests
  adding file changes
  added 8 changesets with 24 changes to 10 files
  updating to branch default
  5 files updated, 0 files merged, 0 files removed, 0 files unresolved
  getting changed largefiles
  3 largefiles updated, 0 removed
  $ cd f
  $ echo "large4-merge-test" > sub/large4
  $ hg commit -m "Modify large4 to test merge"
  $ hg pull ../e
  pulling from ../e
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 4 changes to 4 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hg merge
  merging sub/large4
  largefile sub/large4 has a merge conflict
  keep (l)ocal or take (o)ther? l
  3 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  getting changed largefiles
  1 largefiles updated, 0 removed
  $ hg commit -m "Merge repos e and f"
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
  $ cd ..

vanilla clients not locked out from largefiles servers on vanilla repos
  $ mkdir r1
  $ cd r1
  $ hg init
  $ echo c1 > f1
  $ hg add f1
  $ hg com -m "m1"
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

vanilla clients locked out from largefiles http repos
  $ mkdir r4
  $ cd r4
  $ hg init
  $ echo c1 > f1
  $ hg add --large f1
  $ hg com -m "m1"
  $ cd ..
  $ hg serve -R r4 -d -p $HGPORT2 --pid-file hg.pid
  $ cat hg.pid >> $DAEMON_PIDS
  $ hg --config extensions.largefiles=! clone http://localhost:$HGPORT2 r5
  abort: remote error:
  
  This repository uses the largefiles extension.
  
  Please enable it in your Mercurial config file.
  [255]

used all HGPORTs, kill all daemons
  $ "$TESTDIR/killdaemons.py"

vanilla clients locked out from largefiles ssh repos
  $ hg --config extensions.largefiles=! clone -e "python $TESTDIR/dummyssh" ssh://user@dummy/r4 r5
  abort: remote error:
  
  This repository uses the largefiles extension.
  
  Please enable it in your Mercurial config file.
  [255]

largefiles clients refuse to push largefiles repos to vanilla servers
  $ mkdir r6
  $ cd r6
  $ hg init
  $ echo c1 > f1
  $ hg add f1
  $ hg com -m "m1"
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
  $ hg com -m "m2"
  $ hg --config extensions.largefiles=! -R ../r6 serve -d -p $HGPORT --pid-file ../hg.pid
  $ cat ../hg.pid >> $DAEMON_PIDS
  $ hg push http://localhost:$HGPORT
  pushing to http://localhost:$HGPORT/
  searching for changes
  abort: http://localhost:$HGPORT/ does not appear to be a largefile store
  [255]
  $ cd ..

  $ cd ..

Clone a local repository owned by another user
We have to simulate that here by setting $HOME and removing write permissions
  $ ORIGHOME="$HOME"
  $ mkdir alice
  $ HOME="`pwd`/alice"
  $ cd alice
  $ hg init pubrepo
  $ cd pubrepo
  $ dd if=/dev/urandom bs=1k count=11k > a-large-file 2> /dev/null
  $ hg add --large a-large-file
  $ hg commit -m "Add a large file"
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
  $ HOME="$ORIGHOME"

Symlink to a large largefile should behave the same as a symlink to a normal file
  $ hg init largesymlink
  $ cd largesymlink
  $ dd if=/dev/zero bs=1k count=10k of=largefile 2>/dev/null
  $ hg add --large largefile
  $ hg commit -m "commit a large file"
  $ ln -s largefile largelink
  $ hg add largelink
  $ hg commit -m "commit a large symlink"
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


