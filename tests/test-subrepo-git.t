  $ "$TESTDIR/hghave" git || exit 80

make git commits repeatable

  $ GIT_AUTHOR_NAME='test'; export GIT_AUTHOR_NAME
  $ GIT_AUTHOR_EMAIL='test@example.org'; export GIT_AUTHOR_EMAIL
  $ GIT_AUTHOR_DATE='1234567891 +0000'; export GIT_AUTHOR_DATE
  $ GIT_COMMITTER_NAME="$GIT_AUTHOR_NAME"; export GIT_COMMITTER_NAME
  $ GIT_COMMITTER_EMAIL="$GIT_AUTHOR_EMAIL"; export GIT_COMMITTER_EMAIL
  $ GIT_COMMITTER_DATE="$GIT_AUTHOR_DATE"; export GIT_COMMITTER_DATE

root hg repo

  $ hg init t
  $ cd t
  $ echo a > a
  $ hg add a
  $ hg commit -m a
  $ cd ..

new external git repo

  $ mkdir gitroot
  $ cd gitroot
  $ git init -q
  $ echo g > g
  $ git add g
  $ git commit -q -m g

add subrepo clone

  $ cd ../t
  $ echo 's = [git]../gitroot' > .hgsub
  $ git clone -q ../gitroot s
  $ hg add .hgsub
  $ hg commit -m 'new git subrepo'
  committing subrepository $TESTTMP/t/s
  $ hg debugsub
  path s
   source   ../gitroot
   revision da5f5b1d8ffcf62fb8327bcd3c89a4367a6018e7

record a new commit from upstream from a different branch

  $ cd ../gitroot
  $ git checkout -b testing
  Switched to a new branch 'testing'
  $ echo gg >> g
  $ git commit -q -a -m gg

  $ cd ../t/s
  $ git pull -q
  $ git checkout -b testing origin/testing
  Switched to a new branch 'testing'
  Branch testing set up to track remote branch testing from origin.

  $ cd ..
  $ hg commit -m 'update git subrepo'
  committing subrepository $TESTTMP/t/s
  $ hg debugsub
  path s
   source   ../gitroot
   revision 126f2a14290cd5ce061fdedc430170e8d39e1c5a

clone root

  $ hg clone . ../tc
  updating to branch default
  cloning subrepo s
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ../tc
  $ hg debugsub
  path s
   source   ../gitroot
   revision 126f2a14290cd5ce061fdedc430170e8d39e1c5a

update to previous substate

  $ hg update 1
  Switched to a new branch 'master'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat s/g
  g
  $ hg debugsub
  path s
   source   ../gitroot
   revision da5f5b1d8ffcf62fb8327bcd3c89a4367a6018e7

make $GITROOT pushable, by replacing it with a clone with nothing checked out

  $ cd ..
  $ git clone gitroot gitrootbare --bare -q
  $ rm -rf gitroot
  $ mv gitrootbare gitroot

clone root, make local change

  $ cd t
  $ hg clone . ../ta
  updating to branch default
  cloning subrepo s
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cd ../ta
  $ echo ggg >> s/g
  $ hg commit -m ggg
  committing subrepository $TESTTMP/ta/s
  $ hg debugsub
  path s
   source   ../gitroot
   revision 79695940086840c99328513acbe35f90fcd55e57

clone root separately, make different local change

  $ cd ../t
  $ hg clone . ../tb
  updating to branch default
  cloning subrepo s
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cd ../tb/s
  $ echo f > f
  $ git add f
  $ cd ..

  $ hg commit -m f
  committing subrepository $TESTTMP/tb/s
  $ hg debugsub
  path s
   source   ../gitroot
   revision aa84837ccfbdfedcdcdeeedc309d73e6eb069edc

user b push changes

  $ hg push
  pushing to $TESTTMP/t
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files

user a pulls, merges, commits

  $ cd ../ta
  $ hg pull
  pulling from $TESTTMP/t
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hg merge
  Automatic merge went well; stopped before committing as requested
  pulling subrepo s
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ cat s/f
  f
  $ cat s/g
  g
  gg
  ggg
  $ hg commit -m 'merge'
  committing subrepository $TESTTMP/ta/s
  $ hg debugsub
  path s
   source   ../gitroot
   revision f47b465e1bce645dbf37232a00574aa1546ca8d3
  $ hg push
  pushing to $TESTTMP/t
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 1 files

update to a revision without the subrepo, keeping the local git repository

  $ cd ../t
  $ hg up 0
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ ls s -a
  .
  ..
  .git

  $ hg up 2
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ ls s -a
  .
  ..
  .git
  g
