#require git

make git commits repeatable

  $ echo "[core]" >> $HOME/.gitconfig
  $ echo "autocrlf = false" >> $HOME/.gitconfig
  $ GIT_AUTHOR_NAME='test'; export GIT_AUTHOR_NAME
  $ GIT_AUTHOR_EMAIL='test@example.org'; export GIT_AUTHOR_EMAIL
  $ GIT_AUTHOR_DATE='1234567891 +0000'; export GIT_AUTHOR_DATE
  $ GIT_COMMITTER_NAME="$GIT_AUTHOR_NAME"; export GIT_COMMITTER_NAME
  $ GIT_COMMITTER_EMAIL="$GIT_AUTHOR_EMAIL"; export GIT_COMMITTER_EMAIL
  $ GIT_COMMITTER_DATE="$GIT_AUTHOR_DATE"; export GIT_COMMITTER_DATE
  $ GIT_CONFIG_NOSYSTEM=1; export GIT_CONFIG_NOSYSTEM

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
  $ hg clone . ../tc 2> /dev/null
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
  $ hg clone . ../ta 2> /dev/null
  updating to branch default
  cloning subrepo s from $TESTTMP/gitroot
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cd ../ta
  $ echo ggg >> s/g
  $ hg status --subrepos
  M s/g
  $ hg diff --subrepos
  diff --git a/s/g b/s/g
  index 089258f..85341ee 100644
  --- a/s/g
  +++ b/s/g
  @@ -1,2 +1,3 @@
   g
   gg
  +ggg
  $ hg commit --subrepos -m ggg
  committing subrepository s
  $ hg debugsub
  path s
   source   ../gitroot
   revision 79695940086840c99328513acbe35f90fcd55e57

clone root separately, make different local change

  $ cd ../t
  $ hg clone . ../tb 2> /dev/null
  updating to branch default
  cloning subrepo s from $TESTTMP/gitroot
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cd ../tb/s
  $ hg status --subrepos
  $ echo f > f
  $ hg status --subrepos
  ? s/f
  $ hg add .
  adding f
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
  pushing to $TESTTMP/t (glob)
  pushing branch testing of subrepo s
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files

user a pulls, merges, commits

  $ cd ../ta
  $ hg pull
  pulling from $TESTTMP/t (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hg merge 2>/dev/null
   subrepository s diverged (local revision: 7969594, remote revision: aa84837)
  (M)erge, keep (l)ocal or keep (r)emote? m
  pulling subrepo s from $TESTTMP/gitroot
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg st --subrepos s
  A s/f
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
  pushing to $TESTTMP/t (glob)
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
  $ hg clone . ../td 2>&1 | egrep -v '^Cloning into|^done\.'
  updating to branch default
  cloning subrepo s from $TESTTMP/gitroot
  checking out detached HEAD in subrepo s
  check out a git branch if you intend to make changes
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ../td
  $ echo aa >> a
  $ hg commit -m aa
  $ hg push
  pushing to $TESTTMP/t (glob)
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

create a new git branch

  $ cd s
  $ git checkout -b b2
  Switched to a new branch 'b2'
  $ echo a>a
  $ git add a
  $ git commit -qm 'add a'
  $ cd ..
  $ hg commit -m 'add branch in s'

pulling new git branch should not create tracking branch named 'origin/b2'
(issue3870)
  $ cd ../td/s
  $ git remote set-url origin $TESTTMP/tb/s
  $ git branch --no-track oldtesting
  $ cd ..
  $ hg pull -q ../tb
  $ hg up
  From $TESTTMP/tb/s
   * [new branch]      b2         -> origin/b2
  Previous HEAD position was f47b465... merge
  Switched to a new branch 'b2'
  pulling subrepo s from $TESTTMP/tb/s
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

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

  $ hg -R ../tc archive --subrepo -r 5 -X ../tc/**f ../archive_x 2>/dev/null
  $ find ../archive_x | sort | grep -v pax_global_header
  ../archive_x
  ../archive_x/.hg_archival.txt
  ../archive_x/.hgsub
  ../archive_x/.hgsubstate
  ../archive_x/a
  ../archive_x/s
  ../archive_x/s/g

  $ hg -R ../tc archive -S ../archive.tgz --prefix '.' 2>/dev/null
  $ tar -tzf ../archive.tgz | sort | grep -v pax_global_header
  .hg_archival.txt
  .hgsub
  .hgsubstate
  a
  s/g

create nested repo

  $ cd ..
  $ hg init outer
  $ cd outer
  $ echo b>b
  $ hg add b
  $ hg commit -m b

  $ hg clone ../t inner 2> /dev/null
  updating to branch default
  cloning subrepo s from $TESTTMP/gitroot
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo inner = inner > .hgsub
  $ hg add .hgsub
  $ hg commit -m 'nested sub'

nested commit

  $ echo ffff >> inner/s/f
  $ hg status --subrepos
  M inner/s/f
  $ hg commit --subrepos -m nested
  committing subrepository inner
  committing subrepository inner/s (glob)

nested archive

  $ hg archive --subrepos ../narchive
  $ ls ../narchive/inner/s | grep -v pax_global_header
  f
  g

relative source expansion

  $ cd ..
  $ mkdir d
  $ hg clone t d/t 2> /dev/null
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
  abort: subrepo s is missing (in subrepo s)
  [255]
  $ hg commit --subrepos -qm missing
  abort: subrepo s is missing (in subrepo s)
  [255]

#if symlink
Don't crash if subrepo is a broken symlink
  $ ln -s broken s
  $ hg status -S
  $ hg push -q
  abort: subrepo s is missing (in subrepo s)
  [255]
  $ hg commit --subrepos -qm missing
  abort: subrepo s is missing (in subrepo s)
  [255]
  $ rm s
#endif

  $ hg update -C 2> /dev/null
  cloning subrepo s from $TESTTMP/gitroot
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg sum | grep commit
  commit: (clean)

Don't crash if the .hgsubstate entry is missing

  $ hg update 1 -q
  $ hg rm .hgsubstate
  $ hg commit .hgsubstate -m 'no substate'
  nothing changed
  [1]
  $ hg tag -l nosubstate
  $ hg manifest
  .hgsub
  .hgsubstate
  a

  $ hg status -S
  R .hgsubstate
  $ hg sum | grep commit
  commit: 1 removed, 1 subrepos (new branch head)

  $ hg commit -m 'restore substate'
  nothing changed
  [1]
  $ hg manifest
  .hgsub
  .hgsubstate
  a
  $ hg sum | grep commit
  commit: 1 removed, 1 subrepos (new branch head)

  $ hg update -qC nosubstate
  $ ls s
  g

issue3109: false positives in git diff-index

  $ hg update -q
  $ touch -t 200001010000 s/g
  $ hg status --subrepos
  $ touch -t 200001010000 s/g
  $ hg sum | grep commit
  commit: (clean)

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
  ? s/f2
  $ ls s
  f
  f1
  f2
  g
  $ hg update --clean
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg status -S
  ? s/f1
  ? s/f2
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

Sticky subrepositories, file changes
  $ touch s/f1
  $ cd s
  $ git add f1
  $ cd ..
  $ hg id -n
  1+
  $ cd s
  $ git rev-parse HEAD
  da5f5b1d8ffcf62fb8327bcd3c89a4367a6018e7
  $ cd ..
  $ hg update 4
   subrepository s diverged (local revision: da5f5b1, remote revision: aa84837)
  (M)erge, keep (l)ocal or keep (r)emote? m
   subrepository sources for s differ
  use (l)ocal source (da5f5b1) or (r)emote source (aa84837)? l
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
   subrepository s diverged (local revision: 32a3438, remote revision: da5f5b1)
  (M)erge, keep (l)ocal or keep (r)emote? m
   subrepository sources for s differ (in checked out version)
  use (l)ocal source (32a3438) or (r)emote source (da5f5b1)? l
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
   subrepository s diverged (local revision: 32a3438, remote revision: 32a3438)
  (M)erge, keep (l)ocal or keep (r)emote? m
   subrepository sources for s differ
  use (l)ocal source (32a3438) or (r)emote source (32a3438)? l
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg id -n
  7+
  $ cd s
  $ git rev-parse HEAD
  aa84837ccfbdfedcdcdeeedc309d73e6eb069edc
  $ cd ..

Sticky repository, update --clean
  $ hg update --clean tip 2>/dev/null
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

Test forgetting files, not implemented in git subrepo, used to
traceback
#if no-windows
  $ hg forget 'notafile*'
  notafile*: No such file or directory
  [1]
#else
  $ hg forget 'notafile'
  notafile: * (glob)
  [1]
#endif

  $ cd ..

Test sanitizing ".hg/hgrc" in subrepo

  $ cd t
  $ hg tip -q
  7:af6d2edbb0d3
  $ hg update -q -C af6d2edbb0d3
  $ cd s
  $ git checkout -q -b sanitize-test
  $ mkdir .hg
  $ echo '.hg/hgrc in git repo' > .hg/hgrc
  $ mkdir -p sub/.hg
  $ echo 'sub/.hg/hgrc in git repo' > sub/.hg/hgrc
  $ git add .hg sub
  $ git commit -qm 'add .hg/hgrc to be sanitized at hg update'
  $ git push -q origin sanitize-test
  $ cd ..
  $ grep ' s$' .hgsubstate
  32a343883b74769118bb1d3b4b1fbf9156f4dddc s
  $ hg commit -qm 'commit with git revision including .hg/hgrc'
  $ hg parents -q
  8:3473d20bddcf
  $ grep ' s$' .hgsubstate
  c4069473b459cf27fd4d7c2f50c4346b4e936599 s
  $ cd ..

  $ hg -R tc pull -q
  $ hg -R tc update -q -C 3473d20bddcf 2>&1 | sort
  warning: removing potentially hostile 'hgrc' in '$TESTTMP/tc/s/.hg' (glob)
  warning: removing potentially hostile 'hgrc' in '$TESTTMP/tc/s/sub/.hg' (glob)
  $ cd tc
  $ hg parents -q
  8:3473d20bddcf
  $ grep ' s$' .hgsubstate
  c4069473b459cf27fd4d7c2f50c4346b4e936599 s
  $ test -f s/.hg/hgrc
  [1]
  $ test -f s/sub/.hg/hgrc
  [1]
  $ cd ..

additional test for "git merge --ff" route:

  $ cd t
  $ hg tip -q
  8:3473d20bddcf
  $ hg update -q -C af6d2edbb0d3
  $ cd s
  $ git checkout -q testing
  $ mkdir .hg
  $ echo '.hg/hgrc in git repo' > .hg/hgrc
  $ mkdir -p sub/.hg
  $ echo 'sub/.hg/hgrc in git repo' > sub/.hg/hgrc
  $ git add .hg sub
  $ git commit -qm 'add .hg/hgrc to be sanitized at hg update (git merge --ff)'
  $ git push -q origin testing
  $ cd ..
  $ grep ' s$' .hgsubstate
  32a343883b74769118bb1d3b4b1fbf9156f4dddc s
  $ hg commit -qm 'commit with git revision including .hg/hgrc'
  $ hg parents -q
  9:ed23f7fe024e
  $ grep ' s$' .hgsubstate
  f262643c1077219fbd3858d54e78ef050ef84fbf s
  $ cd ..

  $ cd tc
  $ hg update -q -C af6d2edbb0d3
  $ test -f s/.hg/hgrc
  [1]
  $ test -f s/sub/.hg/hgrc
  [1]
  $ cd ..
  $ hg -R tc pull -q
  $ hg -R tc update -q -C ed23f7fe024e 2>&1 | sort
  warning: removing potentially hostile 'hgrc' in '$TESTTMP/tc/s/.hg' (glob)
  warning: removing potentially hostile 'hgrc' in '$TESTTMP/tc/s/sub/.hg' (glob)
  $ cd tc
  $ hg parents -q
  9:ed23f7fe024e
  $ grep ' s$' .hgsubstate
  f262643c1077219fbd3858d54e78ef050ef84fbf s
  $ test -f s/.hg/hgrc
  [1]
  $ test -f s/sub/.hg/hgrc
  [1]

Test that sanitizing is omitted in meta data area:

  $ mkdir s/.git/.hg
  $ echo '.hg/hgrc in git metadata area' > s/.git/.hg/hgrc
  $ hg update -q -C af6d2edbb0d3
  checking out detached HEAD in subrepo s
  check out a git branch if you intend to make changes

check differences made by most recent change
  $ cd s
  $ cat > foobar << EOF
  > woopwoop
  > 
  > foo
  > bar
  > EOF
  $ git add foobar
  $ cd ..

  $ hg diff --subrepos
  diff --git a/s/foobar b/s/foobar
  new file mode 100644
  index 0000000..8a5a5e2
  --- /dev/null
  +++ b/s/foobar
  @@ -0,0 +1,4 @@
  +woopwoop
  +
  +foo
  +bar

  $ hg commit --subrepos -m "Added foobar"
  committing subrepository s
  created new head

  $ hg diff -c . --subrepos --nodates
  diff -r af6d2edbb0d3 -r 255ee8cf690e .hgsubstate
  --- a/.hgsubstate
  +++ b/.hgsubstate
  @@ -1,1 +1,1 @@
  -32a343883b74769118bb1d3b4b1fbf9156f4dddc s
  +fd4dbf828a5b2fcd36b2bcf21ea773820970d129 s
  diff --git a/s/foobar b/s/foobar
  new file mode 100644
  index 0000000..8a5a5e2
  --- /dev/null
  +++ b/s/foobar
  @@ -0,0 +1,4 @@
  +woopwoop
  +
  +foo
  +bar

check output when only diffing the subrepository
  $ hg diff -c . --subrepos s
  diff --git a/s/foobar b/s/foobar
  new file mode 100644
  index 0000000..8a5a5e2
  --- /dev/null
  +++ b/s/foobar
  @@ -0,0 +1,4 @@
  +woopwoop
  +
  +foo
  +bar

check output when diffing something else
  $ hg diff -c . --subrepos .hgsubstate --nodates
  diff -r af6d2edbb0d3 -r 255ee8cf690e .hgsubstate
  --- a/.hgsubstate
  +++ b/.hgsubstate
  @@ -1,1 +1,1 @@
  -32a343883b74769118bb1d3b4b1fbf9156f4dddc s
  +fd4dbf828a5b2fcd36b2bcf21ea773820970d129 s

add new changes, including whitespace
  $ cd s
  $ cat > foobar << EOF
  > woop    woop
  > 
  > foo
  > bar
  > EOF
  $ echo foo > barfoo
  $ git add barfoo
  $ cd ..

  $ hg diff --subrepos --ignore-all-space
  diff --git a/s/barfoo b/s/barfoo
  new file mode 100644
  index 0000000..257cc56
  --- /dev/null
  +++ b/s/barfoo
  @@ -0,0 +1* @@ (glob)
  +foo
  $ hg diff --subrepos s/foobar
  diff --git a/s/foobar b/s/foobar
  index 8a5a5e2..bd5812a 100644
  --- a/s/foobar
  +++ b/s/foobar
  @@ -1,4 +1,4 @@
  -woopwoop
  +woop    woop
   
   foo
   bar

execute a diffstat
the output contains a regex, because git 1.7.10 and 1.7.11
 change the amount of whitespace
  $ hg diff --subrepos --stat
  \s*barfoo |\s*1 + (re)
  \s*foobar |\s*2 +- (re)
   2 files changed, 2 insertions\(\+\), 1 deletions?\(-\) (re)

adding an include should ignore the other elements
  $ hg diff --subrepos -I s/foobar
  diff --git a/s/foobar b/s/foobar
  index 8a5a5e2..bd5812a 100644
  --- a/s/foobar
  +++ b/s/foobar
  @@ -1,4 +1,4 @@
  -woopwoop
  +woop    woop
   
   foo
   bar

adding an exclude should ignore this element
  $ hg diff --subrepos -X s/foobar
  diff --git a/s/barfoo b/s/barfoo
  new file mode 100644
  index 0000000..257cc56
  --- /dev/null
  +++ b/s/barfoo
  @@ -0,0 +1* @@ (glob)
  +foo

moving a file should show a removal and an add
  $ hg revert --all
  reverting subrepo ../gitroot
  $ cd s
  $ git mv foobar woop
  $ cd ..
  $ hg diff --subrepos
  diff --git a/s/foobar b/s/foobar
  deleted file mode 100644
  index 8a5a5e2..0000000
  --- a/s/foobar
  +++ /dev/null
  @@ -1,4 +0,0 @@
  -woopwoop
  -
  -foo
  -bar
  diff --git a/s/woop b/s/woop
  new file mode 100644
  index 0000000..8a5a5e2
  --- /dev/null
  +++ b/s/woop
  @@ -0,0 +1,4 @@
  +woopwoop
  +
  +foo
  +bar
  $ rm s/woop

revert the subrepository
  $ hg revert --all
  reverting subrepo ../gitroot

  $ hg status --subrepos
  ? s/barfoo
  ? s/foobar.orig

  $ mv s/foobar.orig s/foobar

  $ hg revert --no-backup s
  reverting subrepo ../gitroot

  $ hg status --subrepos
  ? s/barfoo

revert moves orig files to the right place
  $ echo 'bloop' > s/foobar
  $ hg revert --all --verbose --config 'ui.origbackuppath=.hg/origbackups'
  reverting subrepo ../gitroot
  creating directory: $TESTTMP/tc/.hg/origbackups (glob)
  saving current version of foobar as $TESTTMP/tc/.hg/origbackups/foobar.orig (glob)
  $ ls .hg/origbackups
  foobar.orig
  $ rm -rf .hg/origbackups

show file at specific revision
  $ cat > s/foobar << EOF
  > woop    woop
  > fooo bar
  > EOF
  $ hg commit --subrepos -m "updated foobar"
  committing subrepository s
  $ cat > s/foobar << EOF
  > current foobar
  > (should not be visible using hg cat)
  > EOF

  $ hg cat -r . s/foobar
  woop    woop
  fooo bar (no-eol)
  $ hg cat -r "parents(.)" s/foobar > catparents

  $ mkdir -p tmp/s

  $ hg cat -r "parents(.)" --output tmp/%% s/foobar
  $ diff tmp/% catparents

  $ hg cat -r "parents(.)" --output tmp/%s s/foobar
  $ diff tmp/foobar catparents

  $ hg cat -r "parents(.)" --output tmp/%d/otherfoobar s/foobar
  $ diff tmp/s/otherfoobar catparents

  $ hg cat -r "parents(.)" --output tmp/%p s/foobar
  $ diff tmp/s/foobar catparents

  $ hg cat -r "parents(.)" --output tmp/%H s/foobar
  $ diff tmp/255ee8cf690ec86e99b1e80147ea93ece117cd9d catparents

  $ hg cat -r "parents(.)" --output tmp/%R s/foobar
  $ diff tmp/10 catparents

  $ hg cat -r "parents(.)" --output tmp/%h s/foobar
  $ diff tmp/255ee8cf690e catparents

  $ rm tmp/10
  $ hg cat -r "parents(.)" --output tmp/%r s/foobar
  $ diff tmp/10 catparents

  $ mkdir tmp/tc
  $ hg cat -r "parents(.)" --output tmp/%b/foobar s/foobar
  $ diff tmp/tc/foobar catparents

cleanup
  $ rm -r tmp
  $ rm catparents

add git files, using either files or patterns
  $ echo "hsss! hsssssssh!" > s/snake.python
  $ echo "ccc" > s/c.c
  $ echo "cpp" > s/cpp.cpp

  $ hg add s/snake.python s/c.c s/cpp.cpp
  $ hg st --subrepos s
  M s/foobar
  A s/c.c
  A s/cpp.cpp
  A s/snake.python
  ? s/barfoo
  $ hg revert s
  reverting subrepo ../gitroot

  $ hg add --subrepos "glob:**.python"
  adding s/snake.python (glob)
  $ hg st --subrepos s
  A s/snake.python
  ? s/barfoo
  ? s/c.c
  ? s/cpp.cpp
  ? s/foobar.orig
  $ hg revert s
  reverting subrepo ../gitroot

  $ hg add --subrepos s
  adding s/barfoo (glob)
  adding s/c.c (glob)
  adding s/cpp.cpp (glob)
  adding s/foobar.orig (glob)
  adding s/snake.python (glob)
  $ hg st --subrepos s
  A s/barfoo
  A s/c.c
  A s/cpp.cpp
  A s/foobar.orig
  A s/snake.python
  $ hg revert s
  reverting subrepo ../gitroot
make sure everything is reverted correctly
  $ hg st --subrepos s
  ? s/barfoo
  ? s/c.c
  ? s/cpp.cpp
  ? s/foobar.orig
  ? s/snake.python

  $ hg add --subrepos --exclude "path:s/c.c"
  adding s/barfoo (glob)
  adding s/cpp.cpp (glob)
  adding s/foobar.orig (glob)
  adding s/snake.python (glob)
  $ hg st --subrepos s
  A s/barfoo
  A s/cpp.cpp
  A s/foobar.orig
  A s/snake.python
  ? s/c.c
  $ hg revert --all -q

.hgignore should not have influence in subrepos
  $ cat > .hgignore << EOF
  > syntax: glob
  > *.python
  > EOF
  $ hg add .hgignore
  $ hg add --subrepos "glob:**.python" s/barfoo
  adding s/snake.python (glob)
  $ hg st --subrepos s
  A s/barfoo
  A s/snake.python
  ? s/c.c
  ? s/cpp.cpp
  ? s/foobar.orig
  $ hg revert --all -q

.gitignore should have influence,
except for explicitly added files (no patterns)
  $ cat > s/.gitignore << EOF
  > *.python
  > EOF
  $ hg add s/.gitignore
  $ hg st --subrepos s
  A s/.gitignore
  ? s/barfoo
  ? s/c.c
  ? s/cpp.cpp
  ? s/foobar.orig
  $ hg st --subrepos s --all
  A s/.gitignore
  ? s/barfoo
  ? s/c.c
  ? s/cpp.cpp
  ? s/foobar.orig
  I s/snake.python
  C s/f
  C s/foobar
  C s/g
  $ hg add --subrepos "glob:**.python"
  $ hg st --subrepos s
  A s/.gitignore
  ? s/barfoo
  ? s/c.c
  ? s/cpp.cpp
  ? s/foobar.orig
  $ hg add --subrepos s/snake.python
  $ hg st --subrepos s
  A s/.gitignore
  A s/snake.python
  ? s/barfoo
  ? s/c.c
  ? s/cpp.cpp
  ? s/foobar.orig

correctly do a dry run
  $ hg add --subrepos s --dry-run
  adding s/barfoo (glob)
  adding s/c.c (glob)
  adding s/cpp.cpp (glob)
  adding s/foobar.orig (glob)
  $ hg st --subrepos s
  A s/.gitignore
  A s/snake.python
  ? s/barfoo
  ? s/c.c
  ? s/cpp.cpp
  ? s/foobar.orig

error given when adding an already tracked file
  $ hg add s/.gitignore
  s/.gitignore already tracked!
  [1]
  $ hg add s/g
  s/g already tracked!
  [1]

removed files can be re-added
removing files using 'rm' or 'git rm' has the same effect,
since we ignore the staging area
  $ hg ci --subrepos -m 'snake'
  committing subrepository s
  $ cd s
  $ rm snake.python
(remove leftover .hg so Mercurial doesn't look for a root here)
  $ rm -rf .hg
  $ hg status --subrepos --all .
  R snake.python
  ? barfoo
  ? c.c
  ? cpp.cpp
  ? foobar.orig
  C .gitignore
  C f
  C foobar
  C g
  $ git rm snake.python
  rm 'snake.python'
  $ hg status --subrepos --all .
  R snake.python
  ? barfoo
  ? c.c
  ? cpp.cpp
  ? foobar.orig
  C .gitignore
  C f
  C foobar
  C g
  $ touch snake.python
  $ cd ..
  $ hg add s/snake.python
  $ hg status -S
  M s/snake.python
  ? .hgignore
  ? s/barfoo
  ? s/c.c
  ? s/cpp.cpp
  ? s/foobar.orig
  $ hg revert --all -q

make sure we show changed files, rather than changed subtrees
  $ mkdir s/foo
  $ touch s/foo/bwuh
  $ hg add s/foo/bwuh
  $ hg commit -S -m "add bwuh"
  committing subrepository s
  $ hg status -S --change .
  M .hgsubstate
  A s/foo/bwuh
  ? s/barfoo
  ? s/c.c
  ? s/cpp.cpp
  ? s/foobar.orig
  ? s/snake.python.orig

test for Git CVE-2016-3068
  $ hg init malicious-subrepository
  $ cd malicious-subrepository
  $ echo "s = [git]ext::sh -c echo% pwned% >&2" > .hgsub
  $ git init s
  Initialized empty Git repository in $TESTTMP/tc/malicious-subrepository/s/.git/
  $ cd s
  $ git commit --allow-empty -m 'empty'
  [master (root-commit) 153f934] empty
  $ cd ..
  $ hg add .hgsub
  $ hg commit -m "add subrepo"
  $ cd ..
  $ env -u GIT_ALLOW_PROTOCOL hg clone malicious-subrepository malicious-subrepository-protected
  Cloning into '$TESTTMP/tc/malicious-subrepository-protected/s'... (glob)
  fatal: transport 'ext' not allowed
  updating to branch default
  cloning subrepo s from ext::sh -c echo% pwned% >&2
  abort: git clone error 128 in s (in subrepo s)
  [255]

whitelisting of ext should be respected (that's the git submodule behaviour)
  $ env GIT_ALLOW_PROTOCOL=ext hg clone malicious-subrepository malicious-subrepository-clone-allowed
  Cloning into '$TESTTMP/tc/malicious-subrepository-clone-allowed/s'... (glob)
  pwned
  fatal: Could not read from remote repository.
  
  Please make sure you have the correct access rights
  and the repository exists.
  updating to branch default
  cloning subrepo s from ext::sh -c echo% pwned% >&2
  abort: git clone error 128 in s (in subrepo s)
  [255]
