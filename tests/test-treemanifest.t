
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
  dir1\x008b3ffd73f901e83304c83d33132c8e774ceac44ed (esc)
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
  dir1/a (glob)
  dir1/b (glob)
  dir1/dir1/a (glob)
  dir1/dir1/b (glob)
  dir1/dir2/a (glob)
  dir1/dir2/b (glob)
  e

Revision is not created for unchanged directory

  $ mkdir dir2
  $ echo 3 > dir2/a
  $ hg add dir2
  adding dir2/a (glob)
  $ hg debugindex --dir dir1 > before
  $ hg ci -qm 'add dir2'
  $ hg debugindex --dir dir1 > after
  $ diff before after
  $ rm before after

Removing directory does not create an revlog entry

  $ hg rm dir1/dir1
  removing dir1/dir1/a (glob)
  removing dir1/dir1/b (glob)
  $ hg debugindex --dir dir1/dir1 > before
  $ hg ci -qm 'remove dir1/dir1'
  $ hg debugindex --dir dir1/dir1 > after
  $ diff before after
  $ rm before after

Check that hg files (calls treemanifest.walk()) works

  $ hg co 'desc("add dir2")'
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg files -r . dir1
  dir1/a (glob)
  dir1/b (glob)
  dir1/dir1/a (glob)
  dir1/dir1/b (glob)
  dir1/dir2/a (glob)
  dir1/dir2/b (glob)

Check that status between revisions works (calls treemanifest.matches())

  $ hg status --rev 'desc("add dir1")' --rev . dir1
  A dir1/dir1/a
  A dir1/dir1/b
  A dir1/dir2/a
  A dir1/dir2/b

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
  $ hg debugindex --dir dir1
     rev    offset  length   base linkrev nodeid       p1           p2
       0         0      54      0       1 8b3ffd73f901 000000000000 000000000000
       1        54      68      0       2 b66d046c644f 8b3ffd73f901 000000000000
       2       122      12      0       4 b87265673c8a b66d046c644f 000000000000
       3       134      95      0       5 aa5d3adcec72 b66d046c644f 000000000000
       4       229      81      0       6 e29b066b91ad b66d046c644f 000000000000
       5       310     107      5       7 a120ce2b83f5 e29b066b91ad aa5d3adcec72

Merge keeping directory from parent 1 does not create revlog entry. (Note that
dir1's manifest does change, but only because dir1/a's filelog changes.)

  $ hg co 'desc("add dir2")'
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo 8 > dir2/a
  $ hg ci -m 'modify dir2/a'
  created new head

  $ hg debugindex --dir dir2 > before
  $ hg merge 'desc("modify dir1/a")'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg revert -r 'desc("modify dir2/a")' .
  reverting dir1/a (glob)
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
  reverting dir2/a (glob)
  $ hg ci -m 'merge, keeping parent 2'
  created new head
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

Create clone with tree manifests enabled

  $ cd ..
  $ hg clone --pull --config experimental.treemanifest=1 repo-flat repo-mixed
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 4 changesets with 17 changes to 11 files
  updating to branch default
  11 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd repo-mixed
  $ test -f .hg/store/meta
  [1]
  $ grep treemanifest .hg/requires
  treemanifest

Commit should store revlog per directory

  $ hg co 1
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo 3 > a
  $ echo 3 > dir1/a
  $ echo 3 > dir1/dir1/a
  $ hg ci -m 'first tree'
  created new head
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
  created new head
  $ hg diff -r 3

Parent of tree root manifest should be flat manifest, and two for merge

  $ hg debugindex -m
     rev    offset  length   base linkrev nodeid       p1           p2
       0         0      80      0       0 40536115ed9e 000000000000 000000000000
       1        80      83      0       1 f3376063c255 40536115ed9e 000000000000
       2       163     103      0       2 5d9b9da231a2 40536115ed9e 000000000000
       3       266      83      0       3 d17d663cbd8a 5d9b9da231a2 f3376063c255
       4       349     132      4       4 c05a51345f86 f3376063c255 000000000000
       5       481     110      4       5 82594b1f557d 5d9b9da231a2 f3376063c255


Status across flat/tree boundary should work

  $ hg status --rev '.^' --rev .
  M a
  M dir1/a
  M dir1/dir1/a


Turning off treemanifest config has no effect

  $ hg debugindex .hg/store/meta/dir1/00manifest.i
     rev    offset  length   base linkrev nodeid       p1           p2
       0         0     125      0       4 63c9c0557d24 000000000000 000000000000
       1       125     109      0       5 23d12a1f6e0e 000000000000 000000000000
  $ echo 2 > dir1/a
  $ hg --config experimental.treemanifest=False ci -qm 'modify dir1/a'
  $ hg debugindex .hg/store/meta/dir1/00manifest.i
     rev    offset  length   base linkrev nodeid       p1           p2
       0         0     125      0       4 63c9c0557d24 000000000000 000000000000
       1       125     109      0       5 23d12a1f6e0e 000000000000 000000000000
       2       234      55      0       6 3cb2d87b4250 23d12a1f6e0e 000000000000
