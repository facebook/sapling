  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > largefiles=
  > purge=
  > rebase=
  > [largefiles]
  > minsize=2
  > patterns=glob:**.dat
  > EOF

Create the repo with a couple of revisions of both large and normal
files (testing that status correctly shows largefiles.

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

Verify that committing new versions of largefiles results in correct
largefile contents, and also that non-largefiles are not affected
badly.

  $ cat normal1
  normal11
  $ cat large1
  large11
  $ cat sub/normal2
  normal22
  $ cat sub/large2
  large22

Verify removing largefiles and normal files works on largefile repos.
 
  $ hg remove normal1 large1
  $ hg commit -m "remove files"
  $ ls
  sub

Test copying largefiles.

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

Test a separate commit corner case (specifying files to commit) and check
that the commited files have the right value.

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

Test one more commit corner case that has been known to break (comitting from
a sub-directory of the repo).

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

Check that committing standins is not allowed.

  $ cd ..
  $ echo large3 > large3
  $ hg commit .hglf/large3 -m "try to commit standin"
  abort: file ".hglf/large3" is a largefile standin
  (commit the largefile itself instead)
  [255]

Test some cornercases for adding largefiles.

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

Test that files get added as largefiles based on .hgrc settings

  $ echo testdata > test.dat
  $ dd bs=3145728 count=1 if=/dev/zero of=reallylarge > /dev/null 2> /dev/null
  $ hg add
  adding reallylarge as a largefile
  adding test.dat as a largefile
  $ dd bs=1048576 count=1 if=/dev/zero of=reallylarge2 > /dev/null 2> /dev/null

Test that specifying the --lsize command on the comand-line works

  $ hg add --lfsize 1
  adding reallylarge2 as a largefile

Test forget on largefiles.

  $ hg forget large3 large5 test.dat reallylarge reallylarge2
  $ hg st
  A sub2/large6
  A sub2/large7
  R large3
  ? large5
  ? reallylarge
  ? reallylarge2
  ? test.dat
  $ hg commit -m "add/edit more largefiles"
  $ hg st
  ? large3
  ? large5
  ? reallylarge
  ? reallylarge2
  ? test.dat

Test purge with largefiles (verify that largefiles get populated in the
working copy correctly after a purge)

  $ hg purge --all
  $ cat sub/large4
  large44
  $ cat sub2/large6
  large6
  $ cat sub2/large7
  large7

Test cloning a largefiles repo.

  $ cd ..
  $ hg clone a b
  updating to branch default
  5 files updated, 0 files merged, 0 files removed, 0 files unresolved
  getting changed largefiles
  3 largefiles updated, 0 removed
  $ cd b
  $ hg log
  changeset:   7:daea875e9014
  tag:         tip
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
  $ hg log
  changeset:   3:9e8fbc4bce62
  tag:         tip
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
  
  $ cat normal1
  normal22
  $ cat large1
  large22
  $ cat sub/normal2
  normal22
  $ cat sub/large2
  large22

Test that old revisions of a clone have correct largefiles content.  This also
tests update.

  $ hg update -r 1 
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  getting changed largefiles
  1 largefiles updated, 0 removed
  $ cat large1
  large11
  $ cat sub/large2
  large22

Test that rebasing between two repositories does not revert largefiles to old
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

Test rollback on largefiles

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

Test that `update --clean` leaves correct largefiles in working copy.

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

Test that verify --large actaully verifies largefiles

  $ hg verify --large
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  10 files, 10 changesets, 28 total revisions
  searching 1 changesets for largefiles
  verified existence of 3 revisions of 3 largefiles

Test that merging does not revert to old versions of largefiles (this has
also been very problematic).

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
  $ cd ..

Verify that lfconvert adds 'largefiles' to .hg/requires
  $ hg init bigfile-repo
  $ cd bigfile-repo
  $ dd if=/dev/zero bs=1k count=23k > a-large-file 2> /dev/null
  $ hg addremove
  adding a-large-file
  a-large-file: up to 72 MB of RAM may be required to manage this file
  (use 'hg revert a-large-file' to cancel the pending addition)
  $ hg commit -m "Commit file without making it be a largefile"
  $ find .hg/largefiles
  .hg/largefiles
  $ cd ..
  $ hg lfconvert --size 10 bigfile-repo largefiles-repo
  initializing destination largefiles-repo
  $ cat largefiles-repo/.hg/requires
  largefiles
  revlogv1
  fncache
  store
  dotencode
  $ rm -rf bigfile-repo largefiles-repo

