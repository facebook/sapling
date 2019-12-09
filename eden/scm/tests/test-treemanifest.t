#chg-compatible

  $ setconfig extensions.treemanifest=!
#require killdaemons
  $ enable obsstore
  $ cat << EOF >> $HGRCPATH
  > [format]
  > usegeneraldelta=yes
  > [ui]
  > ssh=$PYTHON "$TESTDIR/dummyssh"
  > EOF

Set up repo

  $ hg --config experimental.treemanifest=True init repo
  $ cd repo

Requirements get set on init

  $ grep treemanifest .hg/requires
  treemanifest

Without directories, looks like any other repo

  $ echo 0 > a
  $ echo 0 > b
  $ hg ci -Aqm initial
  $ hg debugdata -m 0
  a\x00362fef284ce2ca02aecc8de6d5e8a1c3af0556fe (esc)
  b\x00362fef284ce2ca02aecc8de6d5e8a1c3af0556fe (esc)

Submanifest is stored in separate revlog

  $ mkdir dir1
  $ echo 1 > dir1/a
  $ echo 1 > dir1/b
  $ echo 1 > e
  $ hg ci -Aqm 'add dir1'
  $ hg debugdata -m 1
  a\x00362fef284ce2ca02aecc8de6d5e8a1c3af0556fe (esc)
  b\x00362fef284ce2ca02aecc8de6d5e8a1c3af0556fe (esc)
  dir1\x008b3ffd73f901e83304c83d33132c8e774ceac44et (esc)
  e\x00b8e02f6433738021a065f94175c7cd23db5f05be (esc)
  $ hg debugdata --dir dir1 0
  a\x00b8e02f6433738021a065f94175c7cd23db5f05be (esc)
  b\x00b8e02f6433738021a065f94175c7cd23db5f05be (esc)

Can add nested directories

  $ mkdir dir1/dir1
  $ echo 2 > dir1/dir1/a
  $ echo 2 > dir1/dir1/b
  $ mkdir dir1/dir2
  $ echo 2 > dir1/dir2/a
  $ echo 2 > dir1/dir2/b
  $ hg ci -Aqm 'add dir1/dir1'
  $ hg files -r .
  a
  b
  dir1/a
  dir1/b
  dir1/dir1/a
  dir1/dir1/b
  dir1/dir2/a
  dir1/dir2/b
  e

The manifest command works

  $ hg manifest
  a
  b
  dir1/a
  dir1/b
  dir1/dir1/a
  dir1/dir1/b
  dir1/dir2/a
  dir1/dir2/b
  e

Revision is not created for unchanged directory

  $ mkdir dir2
  $ echo 3 > dir2/a
  $ hg add dir2
  adding dir2/a
  $ hg debugindex --dir dir1 > before
  $ hg ci -qm 'add dir2'
  $ hg debugindex --dir dir1 > after
  $ diff before after
  $ rm before after

Removing directory does not create an revlog entry

  $ hg rm dir1/dir1
  removing dir1/dir1/a
  removing dir1/dir1/b
  $ hg debugindex --dir dir1/dir1 > before
  $ hg ci -qm 'remove dir1/dir1'
  $ hg debugindex --dir dir1/dir1 > after
  $ diff before after
  $ rm before after

Check that hg files (calls treemanifest.walk()) works
without loading all directory revlogs

  $ hg co 'desc("add dir2")'
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ mv .hg/store/meta/dir2 .hg/store/meta/dir2-backup
  $ hg files -r . dir1
  dir1/a
  dir1/b
  dir1/dir1/a
  dir1/dir1/b
  dir1/dir2/a
  dir1/dir2/b

Check that status between revisions works (calls treemanifest.matches())
without loading all directory revlogs

  $ hg status --rev 'desc("add dir1")' --rev . dir1
  A dir1/dir1/a
  A dir1/dir1/b
  A dir1/dir2/a
  A dir1/dir2/b
  $ mv .hg/store/meta/dir2-backup .hg/store/meta/dir2

Merge creates 2-parent revision of directory revlog

  $ echo 5 > dir1/a
  $ hg ci -Aqm 'modify dir1/a'
  $ hg co '.^'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo 6 > dir1/b
  $ hg ci -Aqm 'modify dir1/b'
  $ hg merge 'desc("modify dir1/a")'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m 'conflict-free merge involving dir1/'
  $ cat dir1/a
  5
  $ cat dir1/b
  6
#if common-zlib
  $ hg debugindex --dir dir1
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      54     -1       1 8b3ffd73f901 000000000000 000000000000
       1        54      68      0       2 68e9d057c5a8 8b3ffd73f901 000000000000
       2       122      12      1       4 4698198d2624 68e9d057c5a8 000000000000
       3       134      55      1       5 44844058ccce 68e9d057c5a8 000000000000
       4       189      55      1       6 bf3d9b744927 68e9d057c5a8 000000000000
       5       244      55      4       7 dde7c0af2a03 bf3d9b744927 44844058ccce
#endif

Merge keeping directory from parent 1 does not create revlog entry. (Note that
dir1's manifest does change, but only because dir1/a's filelog changes.)

  $ hg co 'desc("add dir2")'
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo 8 > dir2/a
  $ hg ci -m 'modify dir2/a'

  $ hg debugindex --dir dir2 > before
  $ hg merge 'desc("modify dir1/a")'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg revert -r 'desc("modify dir2/a")' .
  reverting dir1/a
  $ hg ci -m 'merge, keeping parent 1'
  $ hg debugindex --dir dir2 > after
  $ diff before after
  $ rm before after

Merge keeping directory from parent 2 does not create revlog entry. (Note that
dir2's manifest does change, but only because dir2/a's filelog changes.)

  $ hg co 'desc("modify dir2/a")'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg debugindex --dir dir1 > before
  $ hg merge 'desc("modify dir1/a")'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg revert -r 'desc("modify dir1/a")' .
  reverting dir2/a
  $ hg ci -m 'merge, keeping parent 2'
  $ hg debugindex --dir dir1 > after
  $ diff before after
  $ rm before after

Create flat source repo for tests with mixed flat/tree manifests

  $ cd ..
  $ hg init repo-flat
  $ cd repo-flat

Create a few commits with flat manifest

  $ echo 0 > a
  $ echo 0 > b
  $ echo 0 > e
  $ for d in dir1 dir1/dir1 dir1/dir2 dir2
  > do
  >   mkdir $d
  >   echo 0 > $d/a
  >   echo 0 > $d/b
  > done
  $ hg ci -Aqm initial

  $ echo 1 > a
  $ echo 1 > dir1/a
  $ echo 1 > dir1/dir1/a
  $ hg ci -Aqm 'modify on branch 1'

  $ hg co 0
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo 2 > b
  $ echo 2 > dir1/b
  $ echo 2 > dir1/dir1/b
  $ hg ci -Aqm 'modify on branch 2'

  $ hg merge 1
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m 'merge of flat manifests to new flat manifest'

  $ hg serve -p 0 --port-file $TESTTMP/.port -d --pid-file=hg.pid --errorlog=errors.log
  $ HGPORT=`cat $TESTTMP/.port`
  $ cat hg.pid >> $DAEMON_PIDS

Create clone with tree manifests enabled

  $ cd ..
  $ hg clone --config experimental.treemanifest=1 \
  >   http://localhost:$HGPORT repo-mixed -r 1
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 14 changes to 11 files
  new changesets 5b02a3e8db7e:581ef6037d8b
  updating to branch default
  11 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd repo-mixed
  $ test -d .hg/store/meta
  [1]
  $ grep treemanifest .hg/requires
  treemanifest

Should be possible to push updates from flat to tree manifest repo

  $ hg -R ../repo-flat push ssh://user@dummy/repo-mixed
  pushing to ssh://user@dummy/repo-mixed
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 2 changesets with 3 changes to 3 files

Commit should store revlog per directory

  $ hg co 1
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo 3 > a
  $ echo 3 > dir1/a
  $ echo 3 > dir1/dir1/a
  $ hg ci -m 'first tree'
  $ find .hg/store/meta | sort
  .hg/store/meta
  .hg/store/meta/dir1
  .hg/store/meta/dir1/00manifest.i
  .hg/store/meta/dir1/dir1
  .hg/store/meta/dir1/dir1/00manifest.i
  .hg/store/meta/dir1/dir2
  .hg/store/meta/dir1/dir2/00manifest.i
  .hg/store/meta/dir2
  .hg/store/meta/dir2/00manifest.i

Merge of two trees

  $ hg co 2
  6 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg merge 1
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m 'merge of flat manifests to new tree manifest'
  $ hg diff -r 3

Parent of tree root manifest should be flat manifest, and two for merge

#if common-zlib
  $ hg debugindex -m
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      80     -1       0 40536115ed9e 000000000000 000000000000
       1        80      83      0       1 f3376063c255 40536115ed9e 000000000000
       2       163      89      0       2 5d9b9da231a2 40536115ed9e 000000000000
       3       252      83      2       3 d17d663cbd8a 5d9b9da231a2 f3376063c255
       4       335     124      1       4 51e32a8c60ee f3376063c255 000000000000
       5       459     126      2       5 cc5baa78b230 5d9b9da231a2 f3376063c255
#endif

Status across flat/tree boundary should work

  $ hg status --rev '.^' --rev .
  M a
  M dir1/a
  M dir1/dir1/a


Turning off treemanifest config has no effect

#if common-zlib
  $ hg debugindex --dir dir1
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0     127     -1       4 064927a0648a 000000000000 000000000000
       1       127     111      0       5 25ecb8cb8618 000000000000 000000000000
#endif
  $ echo 2 > dir1/a
  $ hg --config experimental.treemanifest=False ci -qm 'modify dir1/a'
#if common-zlib
  $ hg debugindex --dir dir1
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0     127     -1       4 064927a0648a 000000000000 000000000000
       1       127     111      0       5 25ecb8cb8618 000000000000 000000000000
       2       238      55      1       6 5b16163a30c6 25ecb8cb8618 000000000000
#endif

Stripping and recovering changes should work

  $ hg st --change tip
  M dir1/a
  $ hg debugstrip tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/repo-mixed/.hg/strip-backup/51cfd7b1e13b-78a2f3ed-backup.hg
#if common-zlib
  $ hg debugindex --dir dir1
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0     127     -1       4 064927a0648a 000000000000 000000000000
       1       127     111      0       5 25ecb8cb8618 000000000000 000000000000
#endif
  $ hg incoming .hg/strip-backup/*
  comparing with .hg/strip-backup/*-backup.hg (glob)
  searching for changes
  changeset:   6:51cfd7b1e13b
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     modify dir1/a
  
  $ hg pull .hg/strip-backup/*
  pulling from .hg/strip-backup/51cfd7b1e13b-78a2f3ed-backup.hg
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets 51cfd7b1e13b
  $ hg debugstrip tip
  saved backup bundle to $TESTTMP/repo-mixed/.hg/strip-backup/*-backup.hg (glob)
  $ hg unbundle -q .hg/strip-backup/*
#if common-zlib
  $ hg debugindex --dir dir1
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0     127     -1       4 064927a0648a 000000000000 000000000000
       1       127     111      0       5 25ecb8cb8618 000000000000 000000000000
       2       238      55      1       6 5b16163a30c6 25ecb8cb8618 000000000000
#endif
  $ hg st --change tip
  M dir1/a

Shelving and unshelving should work

  $ echo foo >> dir1/a
  $ hg --config extensions.shelve= shelve
  shelved as default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg --config extensions.shelve= unshelve
  unshelving change 'default'
  $ hg diff --nodates
  diff -r 708a273da119 dir1/a
  --- a/dir1/a
  +++ b/dir1/a
  @@ -1,1 +1,2 @@
   1
  +foo

Pushing from treemanifest repo to an empty repo makes that a treemanifest repo

  $ cd ..
  $ hg init empty-repo
  $ cat << EOF >> empty-repo/.hg/hgrc
  > [experimental]
  > changegroup3=yes
  > EOF
  $ grep treemanifest empty-repo/.hg/requires
  [1]
  $ hg push -R repo -r 0 empty-repo
  pushing to empty-repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 2 changes to 2 files
  $ grep treemanifest empty-repo/.hg/requires
  treemanifest

Pushing to an empty repo works

  $ hg --config experimental.treemanifest=1 init clone
  $ grep treemanifest clone/.hg/requires
  treemanifest
  $ hg push -R repo clone
  pushing to clone
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 11 changesets with 15 changes to 10 files
  $ grep treemanifest clone/.hg/requires
  treemanifest
  $ hg -R clone verify
  checking changesets
  checking manifests
  checking directory manifests
  crosschecking files in changesets and manifests
  checking files
  10 files, 11 changesets, 15 total revisions

Create deeper repo with tree manifests.

  $ hg --config experimental.treemanifest=True init deeprepo
  $ cd deeprepo

  $ mkdir .A
  $ mkdir b
  $ mkdir b/bar
  $ mkdir b/bar/orange
  $ mkdir b/bar/orange/fly
  $ mkdir b/foo
  $ mkdir b/foo/apple
  $ mkdir b/foo/apple/bees

  $ touch .A/one.txt
  $ touch .A/two.txt
  $ touch b/bar/fruits.txt
  $ touch b/bar/orange/fly/gnat.py
  $ touch b/bar/orange/fly/housefly.txt
  $ touch b/foo/apple/bees/flower.py
  $ touch c.txt
  $ touch d.py

  $ hg ci -Aqm 'initial'

  $ echo >> .A/one.txt
  $ echo >> .A/two.txt
  $ echo >> b/bar/fruits.txt
  $ echo >> b/bar/orange/fly/gnat.py
  $ echo >> b/bar/orange/fly/housefly.txt
  $ echo >> b/foo/apple/bees/flower.py
  $ echo >> c.txt
  $ echo >> d.py
  $ hg ci -Aqm 'second'

We'll see that visitdir works by removing some treemanifest revlogs and running
the files command with various parameters.

Test files from the root.

  $ hg files -r .
  .A/one.txt
  .A/two.txt
  b/bar/fruits.txt
  b/bar/orange/fly/gnat.py
  b/bar/orange/fly/housefly.txt
  b/foo/apple/bees/flower.py
  c.txt
  d.py

Excludes with a glob should not exclude everything from the glob's root

  $ hg files -r . -X 'b/fo?' b
  b/bar/fruits.txt
  b/bar/orange/fly/gnat.py
  b/bar/orange/fly/housefly.txt
  $ cp -R .hg/store .hg/store-copy

Test files for a subdirectory.

  $ rm -r .hg/store/meta/~2e_a
  $ hg files -r . b
  b/bar/fruits.txt
  b/bar/orange/fly/gnat.py
  b/bar/orange/fly/housefly.txt
  b/foo/apple/bees/flower.py
  $ hg diff -r '.^' -r . --stat b
   b/bar/fruits.txt              |  1 +
   b/bar/orange/fly/gnat.py      |  1 +
   b/bar/orange/fly/housefly.txt |  1 +
   b/foo/apple/bees/flower.py    |  1 +
   4 files changed, 4 insertions(+), 0 deletions(-)
  $ cp -R .hg/store-copy/. .hg/store

Test files with just includes and excludes.

  $ rm -r .hg/store/meta/~2e_a
  $ rm -r .hg/store/meta/b/bar/orange/fly
  $ rm -r .hg/store/meta/b/foo/apple/bees
  $ hg files -r . -I path:b/bar -X path:b/bar/orange/fly -I path:b/foo -X path:b/foo/apple/bees
  b/bar/fruits.txt
  $ hg diff -r '.^' -r . --stat -I path:b/bar -X path:b/bar/orange/fly -I path:b/foo -X path:b/foo/apple/bees
   b/bar/fruits.txt |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  $ cp -R .hg/store-copy/. .hg/store

Test files for a subdirectory, excluding a directory within it.

  $ rm -r .hg/store/meta/~2e_a
  $ rm -r .hg/store/meta/b/foo
  $ hg files -r . -X path:b/foo b
  b/bar/fruits.txt
  b/bar/orange/fly/gnat.py
  b/bar/orange/fly/housefly.txt
  $ hg diff -r '.^' -r . --stat -X path:b/foo b
   b/bar/fruits.txt              |  1 +
   b/bar/orange/fly/gnat.py      |  1 +
   b/bar/orange/fly/housefly.txt |  1 +
   3 files changed, 3 insertions(+), 0 deletions(-)
  $ cp -R .hg/store-copy/. .hg/store

Test files for a sub directory, including only a directory within it, and
including an unrelated directory.

  $ rm -r .hg/store/meta/~2e_a
  $ rm -r .hg/store/meta/b/foo
  $ hg files -r . -I path:b/bar/orange -I path:a b
  b/bar/orange/fly/gnat.py
  b/bar/orange/fly/housefly.txt
  $ hg diff -r '.^' -r . --stat -I path:b/bar/orange -I path:a b
   b/bar/orange/fly/gnat.py      |  1 +
   b/bar/orange/fly/housefly.txt |  1 +
   2 files changed, 2 insertions(+), 0 deletions(-)
  $ cp -R .hg/store-copy/. .hg/store

Test files for a pattern, including a directory, and excluding a directory
within that.

  $ rm -r .hg/store/meta/~2e_a
  $ rm -r .hg/store/meta/b/foo
  $ rm -r .hg/store/meta/b/bar/orange
  $ hg files -r . glob:**.txt -I path:b/bar -X path:b/bar/orange
  b/bar/fruits.txt
  $ hg diff -r '.^' -r . --stat glob:**.txt -I path:b/bar -X path:b/bar/orange
   b/bar/fruits.txt |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  $ cp -R .hg/store-copy/. .hg/store

Test files with an unusual pattern

  $ hg files -r . 'rootfilesin:b/bar'
  b/bar/fruits.txt

Add some more changes to the deep repo
  $ echo narf >> b/bar/fruits.txt
  $ hg ci -m narf
  $ echo troz >> b/bar/orange/fly/gnat.py
  $ hg ci -m troz

Verify works
  $ hg verify
  checking changesets
  checking manifests
  checking directory manifests
  crosschecking files in changesets and manifests
  checking files
  8 files, 4 changesets, 18 total revisions

Dirlogs are included in fncache
  $ grep meta/.A/00manifest.i .hg/store/fncache
  meta/.A/00manifest.i

Rebuilt fncache includes dirlogs
  $ rm .hg/store/fncache
  $ hg debugrebuildfncache
  adding data/.A/one.txt.i
  adding data/.A/two.txt.i
  adding data/b/bar/fruits.txt.i
  adding data/b/bar/orange/fly/gnat.py.i
  adding data/b/bar/orange/fly/housefly.txt.i
  adding data/b/foo/apple/bees/flower.py.i
  adding data/c.txt.i
  adding data/d.py.i
  adding meta/.A/00manifest.i
  adding meta/b/00manifest.i
  adding meta/b/bar/00manifest.i
  adding meta/b/bar/orange/00manifest.i
  adding meta/b/bar/orange/fly/00manifest.i
  adding meta/b/foo/00manifest.i
  adding meta/b/foo/apple/00manifest.i
  adding meta/b/foo/apple/bees/00manifest.i
  16 items added, 0 removed from fncache

Finish first server
  $ killdaemons.py

Back up the recently added revlogs
  $ cp -R .hg/store .hg/store-newcopy

Verify reports missing dirlog
  $ rm .hg/store/meta/b/00manifest.*
  $ hg verify
  checking changesets
  checking manifests
  checking directory manifests
   0: empty or missing b/
   b/@0: parent-directory manifest refers to unknown revision 67688a370455
   b/@1: parent-directory manifest refers to unknown revision f065da70369e
   b/@2: parent-directory manifest refers to unknown revision ac0d30948e0b
   b/@3: parent-directory manifest refers to unknown revision 367152e6af28
  warning: orphan revlog 'meta/b/bar/00manifest.i'
  warning: orphan revlog 'meta/b/bar/orange/00manifest.i'
  warning: orphan revlog 'meta/b/bar/orange/fly/00manifest.i'
  warning: orphan revlog 'meta/b/foo/00manifest.i'
  warning: orphan revlog 'meta/b/foo/apple/00manifest.i'
  warning: orphan revlog 'meta/b/foo/apple/bees/00manifest.i'
  crosschecking files in changesets and manifests
   b/bar/fruits.txt@0: in changeset but not in manifest
   b/bar/orange/fly/gnat.py@0: in changeset but not in manifest
   b/bar/orange/fly/housefly.txt@0: in changeset but not in manifest
   b/foo/apple/bees/flower.py@0: in changeset but not in manifest
  checking files
  8 files, 4 changesets, 18 total revisions
  6 warnings encountered!
  9 integrity errors encountered!
  (first damaged changeset appears to be 0)
  [1]
  $ cp -R .hg/store-newcopy/. .hg/store

Verify reports missing dirlog entry
  $ mv -f .hg/store-copy/meta/b/00manifest.* .hg/store/meta/b/
  $ hg verify
  checking changesets
  checking manifests
  checking directory manifests
   b/@2: parent-directory manifest refers to unknown revision ac0d30948e0b
   b/@3: parent-directory manifest refers to unknown revision 367152e6af28
   b/bar/@?: rev 2 points to unexpected changeset 2
   b/bar/@?: 44d7e1146e0d not in parent-directory manifest
   b/bar/@?: rev 3 points to unexpected changeset 3
   b/bar/@?: 70b10c6b17b7 not in parent-directory manifest
   b/bar/orange/@?: rev 2 points to unexpected changeset 3
   (expected None)
   b/bar/orange/fly/@?: rev 2 points to unexpected changeset 3
   (expected None)
  crosschecking files in changesets and manifests
  checking files
  8 files, 4 changesets, 18 total revisions
  2 warnings encountered!
  8 integrity errors encountered!
  (first damaged changeset appears to be 2)
  [1]
  $ cp -R .hg/store-newcopy/. .hg/store

Test cloning a treemanifest repo over http.
  $ hg serve -p 0 --port-file $TESTTMP/.port -d --pid-file=hg.pid --errorlog=errors.log
  $ HGPORT=`cat $TESTTMP/.port`
  $ cat hg.pid >> $DAEMON_PIDS
  $ cd ..
We can clone even with the knob turned off and we'll get a treemanifest repo.
  $ hg clone --config experimental.treemanifest=False \
  >   --config experimental.changegroup3=True \
  >   http://localhost:$HGPORT deepclone
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 4 changesets with 18 changes to 8 files
  new changesets 775704be6f52:523e5c631710
  updating to branch default
  8 files updated, 0 files merged, 0 files removed, 0 files unresolved
No server errors.
  $ cat deeprepo/errors.log
requires got updated to include treemanifest
  $ cat deepclone/.hg/requires | grep treemanifest
  treemanifest
Tree manifest revlogs exist.
  $ find deepclone/.hg/store/meta | sort
  deepclone/.hg/store/meta
  deepclone/.hg/store/meta/b
  deepclone/.hg/store/meta/b/00manifest.i
  deepclone/.hg/store/meta/b/bar
  deepclone/.hg/store/meta/b/bar/00manifest.i
  deepclone/.hg/store/meta/b/bar/orange
  deepclone/.hg/store/meta/b/bar/orange/00manifest.i
  deepclone/.hg/store/meta/b/bar/orange/fly
  deepclone/.hg/store/meta/b/bar/orange/fly/00manifest.i
  deepclone/.hg/store/meta/b/foo
  deepclone/.hg/store/meta/b/foo/00manifest.i
  deepclone/.hg/store/meta/b/foo/apple
  deepclone/.hg/store/meta/b/foo/apple/00manifest.i
  deepclone/.hg/store/meta/b/foo/apple/bees
  deepclone/.hg/store/meta/b/foo/apple/bees/00manifest.i
  deepclone/.hg/store/meta/~2e_a
  deepclone/.hg/store/meta/~2e_a/00manifest.i
Verify passes.
  $ cd deepclone
  $ hg verify
  checking changesets
  checking manifests
  checking directory manifests
  crosschecking files in changesets and manifests
  checking files
  8 files, 4 changesets, 18 total revisions
  $ cd ..

Create clones using old repo formats to use in later tests
  $ hg clone --config format.usestore=False \
  >   --config experimental.changegroup3=True \
  >   http://localhost:$HGPORT deeprepo-basicstore
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 4 changesets with 18 changes to 8 files
  new changesets 775704be6f52:523e5c631710
  updating to branch default
  8 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd deeprepo-basicstore
  $ grep store .hg/requires
  [1]
  $ hg serve -p 0 --port-file $TESTTMP/.port -d --pid-file=hg.pid --errorlog=errors.log
  $ HGPORT1=`cat $TESTTMP/.port`
  $ cat hg.pid >> $DAEMON_PIDS
  $ cd ..
  $ hg clone --config format.usefncache=False \
  >   --config experimental.changegroup3=True \
  >   http://localhost:$HGPORT deeprepo-encodedstore
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 4 changesets with 18 changes to 8 files
  new changesets 775704be6f52:523e5c631710
  updating to branch default
  8 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd deeprepo-encodedstore
  $ grep fncache .hg/requires
  [1]
  $ hg serve -p 0 --port-file $TESTTMP/.port -d --pid-file=hg.pid --errorlog=errors.log
  $ HGPORT2=`cat $TESTTMP/.port`
  $ cat hg.pid >> $DAEMON_PIDS
  $ cd ..

Local clone with basicstore
  $ hg clone -U deeprepo-basicstore local-clone-basicstore
  $ hg -R local-clone-basicstore verify
  checking changesets
  checking manifests
  checking directory manifests
  crosschecking files in changesets and manifests
  checking files
  8 files, 4 changesets, 18 total revisions

Local clone with encodedstore
  $ hg clone -U deeprepo-encodedstore local-clone-encodedstore
  $ hg -R local-clone-encodedstore verify
  checking changesets
  checking manifests
  checking directory manifests
  crosschecking files in changesets and manifests
  checking files
  8 files, 4 changesets, 18 total revisions

Local clone with fncachestore
  $ hg clone -U deeprepo local-clone-fncachestore
  $ hg -R local-clone-fncachestore verify
  checking changesets
  checking manifests
  checking directory manifests
  crosschecking files in changesets and manifests
  checking files
  8 files, 4 changesets, 18 total revisions

Stream clone with basicstore
  $ hg clone --config experimental.changegroup3=True --stream -U \
  >   http://localhost:$HGPORT1 stream-clone-basicstore
  streaming all changes
  19 files to transfer, * of data (glob)
  transferred * in * seconds (*) (glob)
  searching for changes
  no changes found
  $ hg -R stream-clone-basicstore verify
  checking changesets
  checking manifests
  checking directory manifests
  crosschecking files in changesets and manifests
  checking files
  8 files, 4 changesets, 18 total revisions

Stream clone with encodedstore
  $ hg clone --config experimental.changegroup3=True --stream -U \
  >   http://localhost:$HGPORT2 stream-clone-encodedstore
  streaming all changes
  19 files to transfer, * of data (glob)
  transferred * in * seconds (*) (glob)
  searching for changes
  no changes found
  $ hg -R stream-clone-encodedstore verify
  checking changesets
  checking manifests
  checking directory manifests
  crosschecking files in changesets and manifests
  checking files
  8 files, 4 changesets, 18 total revisions

Stream clone with fncachestore
  $ hg clone --config experimental.changegroup3=True --stream -U \
  >   http://localhost:$HGPORT stream-clone-fncachestore
  streaming all changes
  19 files to transfer, * of data (glob)
  transferred * in * seconds (*) (glob)
  searching for changes
  no changes found
  $ hg -R stream-clone-fncachestore verify
  checking changesets
  checking manifests
  checking directory manifests
  crosschecking files in changesets and manifests
  checking files
  8 files, 4 changesets, 18 total revisions

Packed bundle
  $ hg -R deeprepo debugcreatestreamclonebundle repo-packed.hg
  writing * bytes for 19 files (glob)
  bundle requirements: generaldelta, revlogv1, treemanifest
  $ hg debugbundle --spec repo-packed.hg
  none-packed1;requirements%3Dgeneraldelta%2Crevlogv1%2Ctreemanifest

Bundle with changegroup2 is not supported

  $ hg -R deeprepo bundle --all -t v2 deeprepo.bundle
  abort: repository does not support bundle version 02
  [255]

Pull does not include changegroup for manifest the client already has from
other branch

  $ mkdir grafted-dir-repo
  $ cd grafted-dir-repo
  $ hg --config experimental.treemanifest=1 init
  $ mkdir dir
  $ echo a > dir/file
  $ echo a > file
  $ hg ci -Am initial
  adding dir/file
  adding file
  $ echo b > dir/file
  $ hg ci -m updated
  $ hg co '.^'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg revert -r tip dir/
  reverting dir/file
  $ echo b > file # to make sure root manifest is sent
  $ hg ci -m grafted
  $ cd ..

  $ hg --config experimental.treemanifest=1 clone --pull -r 1 \
  >   grafted-dir-repo grafted-dir-repo-clone
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 3 changes to 2 files
  new changesets d84f4c419457:09ab742f3b0f
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd grafted-dir-repo-clone
  $ hg pull -r 2
  pulling from $TESTTMP/grafted-dir-repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets 73699489fb7c

Committing a empty commit does not duplicate root treemanifest
  $ echo z >> z
  $ hg commit -Aqm 'pre-empty commit'
  $ hg rm z
  $ hg commit --amend -m 'empty commit'
  $ hg log -r 'tip + tip^' -T '{manifest}\n'
  1:678d3574b88c
  1:678d3574b88c
  $ hg debugstrip -r . -q

Shelve works with treemanifest
  $ echo ababagalamaga >> file
  $ hg shelve --config extensions.shelve= --config experimental.evolution=createmarkers
  shelved as default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
