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
  committing subrepository s
  $ hg debugsub
  path s
   source   ../gitroot
   revision da5f5b1d8ffcf62fb8327bcd3c89a4367a6018e7

record a new commit from upstream from a different branch

  $ cd ../gitroot
  $ git checkout -q -b testing
  $ echo gg >> g
  $ git commit -q -a -m gg

  $ cd ../t/s
  $ git pull -q >/dev/null 2>/dev/null
  $ git checkout -q -b testing origin/testing >/dev/null

  $ cd ..
  $ hg status --subrepos
  M s/g
  $ hg commit -m 'update git subrepo'
  committing subrepository s
  $ hg debugsub
  path s
   source   ../gitroot
   revision 126f2a14290cd5ce061fdedc430170e8d39e1c5a

make $GITROOT pushable, by replacing it with a clone with nothing checked out

  $ cd ..
  $ git clone gitroot gitrootbare --bare -q
  $ rm -rf gitroot
  $ mv gitrootbare gitroot

clone root

  $ cd t
  $ hg clone . ../tc
  updating to branch default
  cloning subrepo s from $TESTTMP/gitroot
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ../tc
  $ hg debugsub
  path s
   source   ../gitroot
   revision 126f2a14290cd5ce061fdedc430170e8d39e1c5a

update to previous substate

  $ hg update 1 -q
  $ cat s/g
  g
  $ hg debugsub
  path s
   source   ../gitroot
   revision da5f5b1d8ffcf62fb8327bcd3c89a4367a6018e7

clone root, make local change

  $ cd ../t
  $ hg clone . ../ta
  updating to branch default
  cloning subrepo s from $TESTTMP/gitroot
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cd ../ta
  $ echo ggg >> s/g
  $ hg status --subrepos
  M s/g
  $ hg commit --subrepos -m ggg
  committing subrepository s
  $ hg debugsub
  path s
   source   ../gitroot
   revision 79695940086840c99328513acbe35f90fcd55e57

clone root separately, make different local change

  $ cd ../t
  $ hg clone . ../tb
  updating to branch default
  cloning subrepo s from $TESTTMP/gitroot
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cd ../tb/s
  $ echo f > f
  $ git add f
  $ cd ..

  $ hg status --subrepos
  A s/f
  $ hg commit --subrepos -m f
  committing subrepository s
  $ hg debugsub
  path s
   source   ../gitroot
   revision aa84837ccfbdfedcdcdeeedc309d73e6eb069edc

user b push changes

  $ hg push 2>/dev/null
  pushing to $TESTTMP/t
  pushing branch testing of subrepo s
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
  $ hg merge 2>/dev/null
  pulling subrepo s from $TESTTMP/gitroot
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ cat s/f
  f
  $ cat s/g
  g
  gg
  ggg
  $ hg commit --subrepos -m 'merge'
  committing subrepository s
  $ hg status --subrepos --rev 1:5
  M .hgsubstate
  M s/g
  A s/f
  $ hg debugsub
  path s
   source   ../gitroot
   revision f47b465e1bce645dbf37232a00574aa1546ca8d3
  $ hg push 2>/dev/null
  pushing to $TESTTMP/t
  pushing branch testing of subrepo s
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 1 files

make upstream git changes

  $ cd ..
  $ git clone -q gitroot gitclone
  $ cd gitclone
  $ echo ff >> f
  $ git commit -q -a -m ff
  $ echo fff >> f
  $ git commit -q -a -m fff
  $ git push origin testing 2>/dev/null

make and push changes to hg without updating the subrepo

  $ cd ../t
  $ hg clone . ../td
  updating to branch default
  cloning subrepo s from $TESTTMP/gitroot
  checking out detached HEAD in subrepo s
  check out a git branch if you intend to make changes
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ../td
  $ echo aa >> a
  $ hg commit -m aa
  $ hg push
  pushing to $TESTTMP/t
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files

sync to upstream git, distribute changes

  $ cd ../ta
  $ hg pull -u -q
  $ cd s
  $ git pull -q >/dev/null 2>/dev/null
  $ cd ..
  $ hg commit -m 'git upstream sync'
  committing subrepository s
  $ hg debugsub
  path s
   source   ../gitroot
   revision 32a343883b74769118bb1d3b4b1fbf9156f4dddc
  $ hg push -q

  $ cd ../tb
  $ hg pull -q
  $ hg update 2>/dev/null
  pulling subrepo s from $TESTTMP/gitroot
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg debugsub
  path s
   source   ../gitroot
   revision 32a343883b74769118bb1d3b4b1fbf9156f4dddc

update to a revision without the subrepo, keeping the local git repository

  $ cd ../t
  $ hg up 0
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ ls -a s
  .
  ..
  .git

  $ hg up 2
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ ls -a s
  .
  ..
  .git
  g

archive subrepos

  $ cd ../tc
  $ hg pull -q
  $ hg archive --subrepos -r 5 ../archive 2>/dev/null
  pulling subrepo s from $TESTTMP/gitroot
  $ cd ../archive
  $ cat s/f
  f
  $ cat s/g
  g
  gg
  ggg

create nested repo

  $ cd ..
  $ hg init outer
  $ cd outer
  $ echo b>b
  $ hg add b
  $ hg commit -m b

  $ hg clone ../t inner
  updating to branch default
  cloning subrepo s from $TESTTMP/gitroot
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo inner = inner > .hgsub
  $ hg add .hgsub
  $ hg commit -m 'nested sub'
  committing subrepository inner

nested commit

  $ echo ffff >> inner/s/f
  $ hg status --subrepos
  M inner/s/f
  $ hg commit --subrepos -m nested
  committing subrepository inner
  committing subrepository inner/s

nested archive

  $ hg archive --subrepos ../narchive
  $ ls ../narchive/inner/s | grep -v pax_global_header
  f
  g

relative source expansion

  $ cd ..
  $ mkdir d
  $ hg clone t d/t
  updating to branch default
  cloning subrepo s from $TESTTMP/gitroot
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

Don't crash if the subrepo is missing

  $ hg clone t missing -q
  $ cd missing
  $ rm -rf s
  $ hg status -S
  $ hg sum | grep commit
  commit: 1 subrepos
  $ hg push -q
  abort: subrepo s is missing
  [255]
  $ hg commit --subrepos -qm missing
  abort: subrepo s is missing
  [255]
  $ hg update -C
  cloning subrepo s from $TESTTMP/gitroot
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg sum | grep commit
  commit: (clean)

Don't crash if the .hgsubstate entry is missing

  $ hg update 1 -q
  $ hg rm .hgsubstate
  $ hg commit .hgsubstate -m 'no substate'
  created new head
  $ hg tag -l nosubstate
  $ hg manifest
  .hgsub
  a

  $ hg status -S
  $ hg sum | grep commit
  commit: 1 subrepos

  $ hg commit -m 'restore substate'
  committing subrepository s
  $ hg manifest
  .hgsub
  .hgsubstate
  a
  $ hg sum | grep commit
  commit: (clean)

  $ hg update -qC nosubstate
  $ ls s

Check hg update --clean
  $ cd $TESTTMP/ta
  $ echo  > s/g
  $ cd s
  $ echo c1 > f1
  $ echo c1 > f2
  $ git add f1
  $ cd ..
  $ hg status -S
  M s/g
  A s/f1
  $ ls s
  f
  f1
  f2
  g
  $ hg update --clean
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg status -S
  $ ls s
  f
  f1
  f2
  g

Sticky subrepositories, no changes
  $ cd $TESTTMP/ta
  $ hg id -n
  7
  $ cd s
  $ git rev-parse HEAD
  32a343883b74769118bb1d3b4b1fbf9156f4dddc
  $ cd ..
  $ hg update 1 > /dev/null 2>&1
  $ hg id -n
  1
  $ cd s
  $ git rev-parse HEAD
  da5f5b1d8ffcf62fb8327bcd3c89a4367a6018e7
  $ cd ..

Sticky subrepositorys, file changes
  $ touch s/f1
  $ cd s
  $ git add f1
  $ cd ..
  $ hg id -n
  1
  $ cd s
  $ git rev-parse HEAD
  da5f5b1d8ffcf62fb8327bcd3c89a4367a6018e7
  $ cd ..
  $ hg update 4
   subrepository sources for s differ
  use (l)ocal source (da5f5b1) or (r)emote source (aa84837)?
   l
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg id -n
  4+
  $ cd s
  $ git rev-parse HEAD
  da5f5b1d8ffcf62fb8327bcd3c89a4367a6018e7
  $ cd ..
  $ hg update --clean tip > /dev/null 2>&1 

Sticky subrepository, revision updates
  $ hg id -n
  7
  $ cd s
  $ git rev-parse HEAD
  32a343883b74769118bb1d3b4b1fbf9156f4dddc
  $ cd ..
  $ cd s
  $ git checkout aa84837ccfbdfedcdcdeeedc309d73e6eb069edc
  Previous HEAD position was 32a3438... fff
  HEAD is now at aa84837... f
  $ cd ..
  $ hg update 1
   subrepository sources for s differ (in checked out version)
  use (l)ocal source (32a3438) or (r)emote source (da5f5b1)?
   l
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg id -n
  1+
  $ cd s
  $ git rev-parse HEAD
  aa84837ccfbdfedcdcdeeedc309d73e6eb069edc
  $ cd ..

Sticky subrepository, file changes and revision updates
  $ touch s/f1
  $ cd s
  $ git add f1
  $ git rev-parse HEAD
  aa84837ccfbdfedcdcdeeedc309d73e6eb069edc
  $ cd ..
  $ hg id -n
  1+
  $ hg update 7
   subrepository sources for s differ
  use (l)ocal source (32a3438) or (r)emote source (32a3438)?
   l
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg id -n
  7
  $ cd s
  $ git rev-parse HEAD
  aa84837ccfbdfedcdcdeeedc309d73e6eb069edc
  $ cd ..

Sticky repository, update --clean
  $ hg update --clean tip
  Previous HEAD position was aa84837... f
  HEAD is now at 32a3438... fff
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg id -n
  7
  $ cd s
  $ git rev-parse HEAD
  32a343883b74769118bb1d3b4b1fbf9156f4dddc
  $ cd ..

Test subrepo already at intended revision:
  $ cd s
  $ git checkout 32a343883b74769118bb1d3b4b1fbf9156f4dddc
  HEAD is now at 32a3438... fff
  $ cd ..
  $ hg update 1
  Previous HEAD position was 32a3438... fff
  HEAD is now at da5f5b1... g
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg id -n
  1
  $ cd s
  $ git rev-parse HEAD
  da5f5b1d8ffcf62fb8327bcd3c89a4367a6018e7
  $ cd ..

