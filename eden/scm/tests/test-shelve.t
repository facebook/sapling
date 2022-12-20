#debugruntest-compatible

# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

  $ setconfig devel.segmented-changelog-rev-compat=true
#if fsmonitor
  $ setconfig workingcopy.ruststatus=False
#endif

  $ cat >> $HGRCPATH << 'EOF'
  > [extensions]
  > strip =
  > shelve=
  > [defaults]
  > diff = --nodates --git
  > qnew = --date '0 0'
  > [shelve]
  > maxbackups = 2
  > [experimental]
  > evolution=createmarkers
  > EOF

# Make sure obs-based shelve can be used with an empty repo

  $ cd "$TESTTMP"
  $ hg init obsrepo
  $ cd obsrepo

  $ mkdir a b
  $ echo a > a/a
  $ echo b > b/b
  $ echo c > c
  $ echo d > d
  $ echo x > x
  $ hg addremove -q
  $ hg shelve
  shelved as default
  0 files updated, 0 files merged, 5 files removed, 0 files unresolved
  $ hg shelve --list
  default * (changes in empty repository) (glob)
  $ hg revert --all
  $ hg unshelve
  unshelving change 'default'
  $ hg diff
  diff --git a/a/a b/a/a
  new file mode 100644
  --- /dev/null
  +++ b/a/a
  @@ -0,0 +1,1 @@
  +a
  diff --git a/b/b b/b/b
  new file mode 100644
  --- /dev/null
  +++ b/b/b
  @@ -0,0 +1,1 @@
  +b
  diff --git a/c b/c
  new file mode 100644
  --- /dev/null
  +++ b/c
  @@ -0,0 +1,1 @@
  +c
  diff --git a/d b/d
  new file mode 100644
  --- /dev/null
  +++ b/d
  @@ -0,0 +1,1 @@
  +d
  diff --git a/x b/x
  new file mode 100644
  --- /dev/null
  +++ b/x
  @@ -0,0 +1,1 @@
  +x
  $ hg ci -qm 'initial commit'
  $ hg shelve
  nothing changed
  [1]

# Make sure shelve files were backed up

  $ ls .hg/shelve-backup
  default.oshelve
  default.patch

  $ echo n > n
  $ hg add n
  $ hg commit n -m second

# Shelve a change that we will delete later

  $ echo a >> a/a
  $ hg shelve
  shelved as default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

# Set up some more complex shelve changes to shelve

  $ echo a >> a/a
  $ hg mv b b.rename
  moving b/b to b.rename/b (glob)
  $ hg cp c c.copy
  $ hg status -C
  M a/a
  A b.rename/b
    b/b
  A c.copy
    c
  R b/b

# The common case - no options or filenames

  $ hg shelve
  shelved as default-01
  2 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg status -C

# Ensure that our shelved changes exist

  $ hg shelve -l
  default-01 * shelve changes to: second (glob)
  default * shelve changes to: second (glob)
  $ hg shelve -l -p default
  default * shelve changes to: second (glob)
  
  diff --git a/a/a b/a/a
  --- a/a/a
  +++ b/a/a
  @@ -1,1 +1,2 @@
   a
  +a

  $ hg shelve --list --addremove
  abort: options '--list' and '--addremove' may not be used together
  [255]

# Delete our older shelved change

  $ hg shelve -d default

# Ensure shelve backups aren't overwritten

  $ ls .hg/shelve-backup/
  default-1.oshelve
  default-1.patch
  default.oshelve
  default.patch

# Local edits should not prevent a shelved change from applying

  $ printf 'z\na\n' > a/a
  $ hg unshelve --keep
  unshelving change 'default-01'
  temporarily committing pending changes (restore with 'hg unshelve --abort')
  rebasing shelved changes
  rebasing * "shelve changes to: second" (glob)
  merging a/a

  $ hg revert --all -q
  $ rm a/a.orig b.rename/b c.copy

# Apply it and make sure our state is as expected
# (this also tests that same timestamp prevents backups from being
# removed, even though there are more than 'maxbackups' backups)

  $ test -f .hg/shelve-backup/default.patch
  $ test -f .hg/shelve-backup/default-1.patch

  $ touch -t 200001010000 .hg/shelve-backup/default.patch
  $ touch -t 200001010000 .hg/shelve-backup/default-1.patch

  $ hg unshelve
  unshelving change 'default-01'
  $ hg status -C
  M a/a
  A b.rename/b
    b/b
  A c.copy
    c
  R b/b
  $ hg shelve -l

# (both of default.oshelve and default-1.oshelve should be still kept,
# because it is difficult to decide actual order of them from same timestamp)

  $ ls .hg/shelve-backup/
  default-01.oshelve
  default-01.patch
  default-1.oshelve
  default-1.patch
  default.oshelve
  default.patch
  $ hg unshelve
  abort: no shelved changes to apply!
  [255]
  $ hg unshelve foo
  abort: shelved change 'foo' not found
  [255]

# Named shelves, specific filenames, and "commit messages" should all work
# (this tests also that editor is invoked, if '--edit' is specified)

  $ hg status -C
  M a/a
  A b.rename/b
    b/b
  A c.copy
    c
  R b/b
  $ HGEDITOR=cat hg shelve -q -n wibble -m wat -e a
  wat
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: test
  HG: branch 'default'
  HG: changed a/a

# Expect "a" to no longer be present, but status otherwise unchanged

  $ hg status -C
  A b.rename/b
    b/b
  A c.copy
    c
  R b/b
  $ hg shelve -l --stat
  wibble * wat (glob)
   a/a |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)

# And now "a/a" should reappear

  $ cd a
  $ hg unshelve -q wibble
  $ cd ..
  $ hg status -C
  M a/a
  A b.rename/b
    b/b
  A c.copy
    c
  R b/b

# Ensure old shelve backups are being deleted automatically

  $ ls .hg/shelve-backup/
  default-01.oshelve
  default-01.patch
  wibble.oshelve
  wibble.patch

# Cause unshelving to result in a merge with 'a' conflicting

  $ hg shelve -q
  $ echo 'c' >> a/a
  $ hg commit -m second
  $ hg tip --template '{files}\n'
  a/a

# Add an unrelated change that should be preserved

  $ mkdir foo
  $ echo foo > foo/foo
  $ hg add foo/foo

# Force a conflicted merge to occur

  $ hg unshelve
  unshelving change 'default'
  temporarily committing pending changes (restore with 'hg unshelve --abort')
  rebasing shelved changes
  rebasing * "shelve changes to: second" (glob)
  merging a/a
  warning: 1 conflicts while merging a/a! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see 'hg resolve', then 'hg unshelve --continue')
  [1]

# Ensure that we have a merge with unresolved conflicts

  $ hg heads -q --template '{rev}\n'
  11
  4
  $ hg parents -q --template '{rev}\n'
  11
  4
  $ hg status
  M a/a
  M b.rename/b
  M c.copy
  R b/b
  ? a/a.orig
  $ hg diff
  diff --git a/a/a b/a/a
  --- a/a/a
  +++ b/a/a
  @@ -1,2 +1,6 @@
   a
  +<<<<<<< dest:   * - test: pending changes temporary commit (glob)
   c
  +=======
  +a
  +>>>>>>> source: * - test: shelve changes to: second (glob)
  diff --git a/b/b b/b.rename/b
  rename from b/b
  rename to b.rename/b
  diff --git a/c b/c.copy
  copy from c
  copy to c.copy
  $ hg resolve -l
  U a/a

  $ hg shelve
  abort: unshelve already in progress
  (use 'hg unshelve --continue' or 'hg unshelve --abort')
  [255]

# Abort the unshelve and be happy

  $ hg status
  M a/a
  M b.rename/b
  M c.copy
  R b/b
  ? a/a.orig
  $ hg unshelve -a
  rebase aborted
  unshelve of 'default' aborted
  $ hg heads -q
  c2e78cacc5ac
  $ hg parents -T '{node|short}\n'
  c2e78cacc5ac
  $ hg resolve -l
  $ hg status
  A foo/foo
  ? a/a.orig

# Try to continue with no unshelve underway

  $ hg unshelve -c
  abort: no unshelve in progress
  [255]
  $ hg status
  A foo/foo
  ? a/a.orig

# Redo the unshelve to get a conflict

  $ hg unshelve -q
  warning: 1 conflicts while merging a/a! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see 'hg resolve', then 'hg unshelve --continue')
  [1]

# Attempt to continue

  $ hg unshelve -c
  abort: unresolved conflicts, can't continue
  (see 'hg resolve', then 'hg unshelve --continue')
  [255]
  $ hg revert -r . a/a
  $ hg resolve -m a/a
  (no more unresolved files)
  continue: hg unshelve --continue
  $ hg commit -m 'commit while unshelve in progress'
  abort: unshelve already in progress
  (use 'hg unshelve --continue' or 'hg unshelve --abort')
  [255]
  $ hg graft --continue
  abort: no graft in progress
  (continue: hg unshelve --continue)
  [255]
  $ hg unshelve -c --trace
  rebasing * "shelve changes to: second" (glob)
  unshelve of 'default' complete

# Ensure the repo is as we hope

  $ hg parents -T '{node|short}\n'
  c2e78cacc5ac
  $ hg heads -q
  201e9c39b40b
  $ hg status -C
  A b.rename/b
    b/b
  A c.copy
    c
  A foo/foo
  R b/b
  ? a/a.orig

# There should be no shelves left

  $ hg shelve -l

#if execbit
# Ensure that metadata-only changes are shelved

  $ chmod +x a/a

  $ hg shelve -q -n execbit a/a
  $ hg status a/a
  $ hg unshelve -q execbit
  $ hg status a/a
  M a/a
  $ hg revert a/a
#endif

#if symlink
# Ensure symlinks are properly handled
  $ rm a/a
  $ ln -s foo a/a
  $ hg shelve -q -n symlink a/a
  $ hg status a/a
  $ hg unshelve -q symlink
  $ hg status a/a
  M a/a
  $ hg revert a/a
#endif

# Set up another conflict between a commit and a shelved change

  $ hg revert -q -C -a
  $ rm a/a.orig b.rename/b c.copy
  $ echo a >> a/a
  $ hg shelve -q
  $ echo x >> a/a
  $ hg ci -m 'create conflict'
  $ hg add foo/foo

# If we resolve a conflict while unshelving, the unshelve should succeed

  $ hg unshelve --tool ':merge-other' --keep
  unshelving change 'default'
  temporarily committing pending changes (restore with 'hg unshelve --abort')
  rebasing shelved changes
  rebasing .* "shelve changes to: second" (re)
  merging a/a
  $ hg shelve -l
  default * shelve changes to: second (glob)
  $ hg status
  M a/a
  A foo/foo
  $ cat a/a
  a
  c
  a
  $ cat > a/a << 'EOF'
  > a
  > c
  > x
  > EOF
  $ HGMERGE=true hg unshelve
  unshelving change 'default'
  temporarily committing pending changes (restore with 'hg unshelve --abort')
  rebasing shelved changes
  rebasing * "shelve changes to: second" (glob)
  merging a/a
  note: rebase of * created no changes to commit (glob)
  $ hg shelve -l
  $ hg status
  M a/a
  A foo/foo
  $ cat a/a
  a
  c
  a

# Test keep and cleanup

  $ hg shelve
  shelved as default
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg shelve --list
  default * shelve changes to: create conflict (glob)
  $ hg unshelve -k
  unshelving change 'default'
  $ hg shelve --list
  default * shelve changes to: create conflict (glob)
  $ hg shelve --cleanup
  $ hg shelve --list

# Test bookmarks

  $ hg bookmark test
  $ hg bookmark
  * test                      * (glob)
  $ hg shelve
  shelved as test
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg bookmark
  * test                      * (glob)
  $ hg unshelve
  unshelving change 'test'
  $ hg bookmark
  * test                      * (glob)

# Shelve should still work even if mq is disabled

  $ hg --config 'extensions.mq=!' shelve
  shelved as test
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg --config 'extensions.mq=!' shelve --list
  test * shelve changes to: create conflict (glob)
  $ hg bookmark
  * test                      * (glob)
  $ hg --config 'extensions.mq=!' unshelve
  unshelving change 'test'
  $ hg bookmark
  * test                      * (glob)
  $ cd ..

# Shelve should leave dirstate clean (issue4055)

  $ hg init obsshelverebase
  $ cd obsshelverebase
  $ printf 'x\ny\n' > x
  $ echo z > z
  $ hg commit -Aqm xy
  $ echo z >> x
  $ hg commit -Aqm z
  $ hg up 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ printf 'a\nx\ny\nz\n' > x
  $ hg commit -Aqm xyz
  $ echo c >> z
  $ hg shelve
  shelved as default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg rebase -d 1 --config 'extensions.rebase='
  rebasing 323bfa07f744 "xyz"
  merging x
  rebasing a2281b51947d "shelve changes to: xyz"
  $ hg unshelve
  unshelving change 'default'
  rebasing shelved changes
  rebasing a2281b51947d "shelve changes to: xyz"
  $ hg status
  M z
  $ cd ..

# Shelve should only unshelve pending changes (issue4068)

  $ hg init obssh-onlypendingchanges
  $ cd obssh-onlypendingchanges
  $ touch a
  $ hg ci -Aqm a
  $ touch b
  $ hg ci -Aqm b
  $ hg up -q 0
  $ touch c
  $ hg ci -Aqm c
  $ touch d
  $ hg add d
  $ hg shelve
  shelved as default
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg up -q 1
  $ hg unshelve
  unshelving change 'default'
  rebasing shelved changes
  rebasing 7eac9d98447f "shelve changes to: c"
  $ hg status
  A d

# Unshelve should work on an ancestor of the original commit

  $ hg shelve
  shelved as default
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg unshelve
  unshelving change 'default'
  rebasing shelved changes
  rebasing 325b64d70042 "shelve changes to: b"
  $ hg status
  A d

# Unshelve should leave unknown files alone (issue4113)

  $ echo e > e
  $ hg shelve
  shelved as default
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg status
  ? e
  $ hg unshelve
  unshelving change 'default'
  $ hg status
  A d
  ? e
  $ cat e
  e

# 139. Unshelve should keep a copy of unknown files

  $ hg add e
  $ hg shelve
  shelved as default
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ echo z > e
  $ hg unshelve
  unshelving change 'default'
  $ cat e
  e
  $ cat e.orig
  z

# 140. Unshelve and conflicts with tracked and untracked files
#  preparing:

  $ rm 'e.orig'
  $ hg ci -qm 'commit stuff'
  $ hg debugmakepublic 'null:'

#  no other changes - no merge:

  $ echo f > f
  $ hg add f
  $ hg shelve
  shelved as default
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo g > f
  $ hg unshelve
  unshelving change 'default'
  $ hg st
  A f
  ? f.orig
  $ cat f
  f
  $ cat f.orig
  g

#  other uncommitted changes - merge:

  $ hg st
  A f
  ? f.orig
  $ hg shelve
  shelved as default
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg log -G --template '{rev}  {desc|firstline}  {author}'
  @  9  commit stuff  test
  │
  │ o  8  shelve changes to: a  test
  ├─╯
  │ o  7  shelve changes to: a  test
  ├─╯
  │ o  6  shelve changes to: b  test
  ├─╯
  │ o  5  shelve changes to: b  test
  │ │
  │ │ o  4  shelve changes to: c  test
  │ ├─╯
  │ │ o  3  shelve changes to: c  test
  │ │ │
  │ │ o  2  c  test
  ├───╯
  │ o  1  b  test
  ├─╯
  o  0  a  test
  $ mv f.orig f
  $ echo 1 > a
  $ hg unshelve --date '1073741824 0'
  unshelving change 'default'
  temporarily committing pending changes (restore with 'hg unshelve --abort')
  rebasing shelved changes
  rebasing a0cc43106cdd "shelve changes to: commit stuff"
  merging f
  warning: 1 conflicts while merging f! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see 'hg resolve', then 'hg unshelve --continue')
  [1]
  $ hg parents -T '{desc|firstline}\n'
  pending changes temporary commit
  shelve changes to: commit stuff

  $ hg st
  M f
  ? f.orig
  $ cat f
  <<<<<<< dest:   f53a8a3b0fad - test: pending changes temporary commit
  g
  =======
  f
  >>>>>>> source: a0cc43106cdd - test: shelve changes to: commit stuff
  $ cat f.orig
  g
  $ hg unshelve --abort -t false
  tool option will be ignored
  rebase aborted
  unshelve of 'default' aborted
  $ hg st
  M a
  ? f.orig
  $ cat f.orig
  g
  $ hg unshelve
  unshelving change 'default'
  temporarily committing pending changes (restore with 'hg unshelve --abort')
  rebasing shelved changes
  rebasing a0cc43106cdd "shelve changes to: commit stuff"
  $ hg st
  M a
  A f
  ? f.orig

#  other committed changes - merge:

  $ hg shelve f
  shelved as default
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg ci a -m 'intermediate other change'
  $ mv f.orig f
  $ hg unshelve
  unshelving change 'default'
  rebasing shelved changes
  rebasing a0cc43106cdd "shelve changes to: commit stuff"
  merging f
  warning: 1 conflicts while merging f! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see 'hg resolve', then 'hg unshelve --continue')
  [1]
  $ hg st
  M f
  ? f.orig
  $ cat f
  <<<<<<< dest:   * - test: intermediate other change (glob)
  g
  =======
  f
  >>>>>>> source: a0cc43106cdd - test: shelve changes to: commit stuff
  $ cat f.orig
  g
  $ hg unshelve --abort
  rebase aborted
  unshelve of 'default' aborted
  $ hg st
  ? f.orig
  $ cat f.orig
  g
  $ hg shelve --delete default

# Recreate some conflict again

  $ cd ../obsrepo
  $ hg up -C -r 'test^'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark test)
  $ echo y >> a/a
  $ hg shelve
  shelved as default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg up test
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark test)
  $ hg bookmark
  * test                      * (glob)
  $ hg unshelve
  unshelving change 'default'
  rebasing shelved changes
  rebasing * "shelve changes to: second" (glob)
  merging a/a
  warning: 1 conflicts while merging a/a! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see 'hg resolve', then 'hg unshelve --continue')
  [1]
  $ hg bookmark
     test * (glob)

# Test that resolving all conflicts in one direction (so that the rebase
# is a no-op), works (issue4398)

  $ hg revert -a -r .
  reverting a/a (glob)
  $ hg resolve -m a/a
  (no more unresolved files)
  continue: hg unshelve --continue
  $ hg unshelve -c
  rebasing * "shelve changes to: second" (glob)
  note: rebase of * created no changes to commit (glob)
  unshelve of 'default' complete
  $ hg bookmark
  * test                      * (glob)
  $ hg diff
  $ hg status
  ? a/a.orig
  ? foo/foo

  $ hg shelve --delete --stat
  abort: options '--delete' and '--stat' may not be used together
  [255]
  $ hg shelve --delete --name NAME
  abort: options '--delete' and '--name' may not be used together
  [255]

# Test interactive shelve

  $ cat >> $HGRCPATH << 'EOF'
  > [ui]
  > interactive = true
  > EOF
  $ echo a >> a/b
  $ cat a/a >> a/b
  $ echo x >> a/b
  $ mv a/b a/a
  $ echo a >> foo/foo
  $ hg st
  M a/a
  ? a/a.orig
  ? foo/foo
  $ cat a/a
  a
  a
  c
  x
  x
  $ cat foo/foo
  foo
  a
  $ hg shelve --interactive --config 'ui.interactive=false'
  abort: running non-interactively
  [255]
  $ hg shelve --interactive << 'EOS'
  > y
  > y
  > n
  > EOS
  diff --git a/a/a b/a/a
  2 hunks, 2 lines changed
  examine changes to 'a/a'? [Ynesfdaq?] y
  
  @@ -1,3 +1,4 @@
   a
  +a
   c
   x
  record change 1/2 to 'a/a'? [Ynesfdaq?] y
  
  @@ -2,2 +3,3 @@
   c
   x
  +x
  record change 2/2 to 'a/a'? [Ynesfdaq?] n
  
  shelved as test
  merging a/a
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  $ cat a/a
  a
  c
  x
  x
  $ cat foo/foo
  foo
  a
  $ hg st
  M a/a
  ? foo/foo
  $ hg bookmark
  * test                      * (glob)
  $ hg log -r . -T '{desc|firstline}\n'
  create conflict
  $ hg unshelve
  unshelving change 'test'
  temporarily committing pending changes (restore with 'hg unshelve --abort')
  rebasing shelved changes
  rebasing * "shelve changes to: create conflict" (glob)
  merging a/a
  $ hg bookmark
  * test                      * (glob)
  $ hg log -r . -T '{desc|firstline}\n'
  create conflict
  $ cat a/a
  a
  a
  c
  x
  x

# Shelve --patch and shelve --stat should work with a single valid shelfname

  $ hg up --clean .
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark test)
  $ hg shelve --list
  $ echo 'patch a' > shelf-patch-a
  $ hg add shelf-patch-a
  $ hg shelve
  shelved as default
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo 'patch b' > shelf-patch-b
  $ hg add shelf-patch-b
  $ hg shelve
  shelved as default-01
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg shelve --patch default default-01
  default-01 * shelve changes to: create conflict (glob)
  
  diff --git a/shelf-patch-b b/shelf-patch-b
  new file mode 100644
  --- /dev/null
  +++ b/shelf-patch-b
  @@ -0,0 +1,1 @@
  +patch b
  default * shelve changes to: create conflict (glob)
  
  diff --git a/shelf-patch-a b/shelf-patch-a
  new file mode 100644
  --- /dev/null
  +++ b/shelf-patch-a
  @@ -0,0 +1,1 @@
  +patch a
  $ hg shelve --stat default default-01
  default-01 * shelve changes to: create conflict (glob)
   shelf-patch-b |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  default * shelve changes to: create conflict (glob)
   shelf-patch-a |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  $ hg shelve --patch default
  default * shelve changes to: create conflict (glob)
  
  diff --git a/shelf-patch-a b/shelf-patch-a
  new file mode 100644
  --- /dev/null
  +++ b/shelf-patch-a
  @@ -0,0 +1,1 @@
  +patch a

# No-argument --patch should also work

  $ hg shelve --patch
  default-01      (*)*shelve changes to: create conflict (glob)
  
  diff --git a/shelf-patch-b b/shelf-patch-b
  new file mode 100644
  --- /dev/null
  +++ b/shelf-patch-b
  @@ -0,0 +1,1 @@
  +patch b
  $ hg shelve --stat default
  default * shelve changes to: create conflict (glob)
   shelf-patch-a |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  $ hg shelve --patch nonexistentshelf
  abort: cannot find shelf nonexistentshelf
  [255]
  $ hg shelve --stat nonexistentshelf
  abort: cannot find shelf nonexistentshelf
  [255]

# Test .orig files go where the user wants them to
# ---------------------------------------------------------------

  $ newrepo obssh-salvage
  $ echo content > root
  $ hg commit -A -m root -q
  $ echo '' > root
  $ hg shelve -q
  $ echo contADDent > root
  $ hg unshelve -q --config 'ui.origbackuppath=.hg/origbackups'
  warning: 1 conflicts while merging root! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see 'hg resolve', then 'hg unshelve --continue')
  [1]
  $ ls .hg/origbackups
  root
  $ rm -rf .hg/origbackups

# Test Abort unshelve always gets user out of the unshelved state
# ---------------------------------------------------------------
# Wreak havoc on the unshelve process

  $ rm .hg/unshelverebasestate

  $ hg unshelve --abort
  unshelve of 'default' aborted
  abort: $ENOENT$: $TESTTMP/obssh-salvage/.hg/unshelverebasestate
  [255]

# Can the user leave the current state?

  $ hg up -C .
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

# Try again but with a corrupted shelve state file

  $ hg up -r 0 -q
  $ echo '' > root
  $ hg shelve -q
  $ echo contADDent > root
  $ hg unshelve -q
  warning: 1 conflicts while merging root! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see 'hg resolve', then 'hg unshelve --continue')
  [1]

  $ sed 's/ae8c668541e8/123456789012/' .hg/shelvedstate > ../corrupt-shelvedstate
  $ mv ../corrupt-shelvedstate .hg/histedit-state

  $ hg unshelve --abort
  rebase aborted
  unshelve of * aborted (glob)
  $ hg up -C .
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ..

# Keep active bookmark while (un)shelving even on shared repo (issue4940)
# -----------------------------------------------------------------------

  $ cat >> $HGRCPATH << 'EOF'
  > [extensions]
  > share =
  > [experimnetal]
  > evolution=createmarkers
  > EOF
  $ hg bookmarks -R obsrepo
     test * (glob)
  $ hg share -B obsrepo obsshare
  updating working directory
  6 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd obsshare

  $ hg bookmarks
     test                      * (glob)
  $ hg bookmarks foo
  $ hg bookmarks
   * foo                       * (glob)
     test                      * (glob)
  $ echo x >> x
  $ hg shelve
  shelved as foo
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bookmarks
   * foo                       * (glob)
     test                      * (glob)

  $ hg unshelve
  unshelving change 'foo'
  $ hg bookmarks
   * foo                       * (glob)
     test                      * (glob)

  $ cd ..

# Shelve and unshelve unknown files. For the purposes of unshelve, a shelved
# unknown file is the same as a shelved added file, except that it will be in
# unknown state after unshelve if and only if it was either absent or unknown
# before the unshelve operation.

  $ hg init obssh-unknowns
  $ cd obssh-unknowns

# The simplest case is if I simply have an unknown file that I shelve and unshelve

  $ echo unknown > unknown
  $ hg status
  ? unknown
  $ hg shelve --unknown
  shelved as default
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg status
  $ hg unshelve
  unshelving change 'default'
  $ hg status
  ? unknown
  $ rm unknown

# If I shelve, add the file, and unshelve, does it stay added?

  $ echo unknown > unknown
  $ hg shelve -u
  shelved as default
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg status
  $ touch unknown
  $ hg add unknown
  $ hg status
  A unknown
  $ hg unshelve
  unshelving change 'default'
  temporarily committing pending changes (restore with 'hg unshelve --abort')
  rebasing shelved changes
  rebasing c850bce25d9f "(changes in empty repository)"
  merging unknown
  $ hg status
  A unknown
  $ hg forget unknown
  $ rm unknown

# And if I shelve, commit, then unshelve, does it become modified?

  $ echo unknown > unknown
  $ hg shelve -u
  shelved as default
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg status
  $ touch unknown
  $ hg add unknown
  $ hg commit -qm 'Add unknown'
  $ hg status
  $ hg unshelve
  unshelving change 'default'
  rebasing shelved changes
  rebasing c850bce25d9f "(changes in empty repository)"
  merging unknown
  $ hg status
  M unknown
  $ hg remove --force unknown
  $ hg commit -qm 'Remove unknown'
  $ cd ..

# Prepare unshelve with a corrupted shelvedstate

  $ hg init obssh-r1
  $ cd obssh-r1
  $ echo text1 > file
  $ hg add file
  $ hg shelve
  shelved as default
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo text2 > file
  $ hg ci -Am text1
  adding file
  $ hg unshelve
  unshelving change 'default'
  rebasing shelved changes
  rebasing a6a994ce5ac2 "(changes in empty repository)"
  merging file
  warning: 1 conflicts while merging file! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see 'hg resolve', then 'hg unshelve --continue')
  [1]
  $ echo somethingsomething > .hg/shelvedstate

# Unshelve --continue fails with appropriate message if shelvedstate is corrupted

  $ hg continue
  abort: corrupted shelved state file
  (please run hg unshelve --abort to abort unshelve operation)
  [255]

# Unshelve --abort works with a corrupted shelvedstate

  $ hg unshelve --abort
  could not read shelved state file, your working copy may be in an unexpected state
  please update to some commit

# Unshelve --abort fails with appropriate message if there's no unshelve in
# progress

  $ hg unshelve --abort
  abort: no unshelve in progress
  [255]
  $ cd ..

# Unshelve respects --keep even if user intervention is needed

  $ hg init obs-unshelvekeep
  $ cd obs-unshelvekeep
  $ echo 1 > file
  $ hg ci -Am 1
  adding file
  $ echo 2 >> file
  $ hg shelve
  shelved as default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo 3 > file
  $ hg ci -Am 13
  $ hg shelve --list
  default * shelve changes to: 1 (glob)
  $ hg unshelve --keep
  unshelving change 'default'
  rebasing shelved changes
  rebasing 49351a7ca591 "shelve changes to: 1"
  merging file
  warning: 1 conflicts while merging file! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see 'hg resolve', then 'hg unshelve --continue')
  [1]
  $ hg resolve --mark file
  (no more unresolved files)
  continue: hg unshelve --continue
  $ hg unshelve --continue
  rebasing 49351a7ca591 "shelve changes to: 1"
  unshelve of 'default' complete
  $ hg shelve --list
  default * shelve changes to: 1 (glob)
  $ cd ..

# Unshelving a stripped commit aborts with an explanatory message

  $ hg init obs-unshelve-stripped-commit
  $ cd obs-unshelve-stripped-commit
  $ echo 1 > file
  $ hg ci -Am 1
  adding file
  $ echo 2 >> file
  $ hg shelve
  shelved as default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg debugstrip -r 1 --config 'experimental.evolution=!' --hidden
  $ hg unshelve
  unshelving change 'default'
  abort: shelved node 49351a7ca59142b32064896a48f50bdecccf8ea0 not found in repo
  If you think this shelve should exist, try running 'hg import --no-commit .hg/shelved/default.patch' from the root of the repository.
  [255]
  $ cd ..

# Test revsetpredicate 'shelved'
# For this test enabled shelve extension is enough, and it is enabled at the top of the file

  $ hg init test-log-shelved
  $ cd test-log-shelved

  $ testshelvedcount() {
  >   local count=$(hg log -r "shelved()" -T "{node}\n" | wc -l)
  >   [ $count -eq $1 ]
  > }

  $ touch file1
  $ touch file2
  $ touch file3
  $ hg addremove
  adding file1
  adding file2
  adding file3
  $ hg commit -m 'Add test files'
  $ echo 1 >> file1
  $ hg shelve
  shelved as default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ testshelvedcount 1
  $ echo 2 >> file2
  $ hg shelve
  shelved as default-01
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ testshelvedcount 2
  $ echo 3 >> file3
  $ hg shelve
  shelved as default-02
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ testshelvedcount 3
  $ hg log --hidden -r 'shelved()' --template '{node|short} {shelvename}\n'
  d7a61836580c default
  9dcce8f0ff7d default-01
  225e1bca0190 default-02
  $ hg unshelve > /dev/null
  $ testshelvedcount 2
  $ hg unshelve > /dev/null
  $ testshelvedcount 1
  $ hg unshelve > /dev/null
  $ testshelvedcount 0
  $ cd ..

# Test interrupted shelve - this should not lose work

  $ newrepo
  $ echo 1 > file1
  $ echo 1 > file2
  $ hg commit -Aqm commit1
  $ echo 2 > file2

  $ cat file2
  2
  $ tglog
  @  6408d34d8180 'commit1'

  $ cat >> $TESTTMP/abortupdate.py << 'EOF'
  > from edenscm import extensions, hg
  > def update(orig, repo, *args, **kwargs):
  >     if not repo.ui.configbool("abortupdate", "abort"):
  >         return orig(repo, *args, **kwargs)
  >     if repo.ui.configbool("abortupdate", "after"):
  >         orig(repo, *args, **kwargs)
  >     raise KeyboardInterrupt
  > def extsetup(ui):
  >     extensions.wrapfunction(hg, "update", update)
  > EOF

  $ setconfig extensions.abortcreatemarkers="$TESTTMP/abortupdate.py"
  $ hg shelve --config abortupdate.abort=true
  shelved as default
  interrupted!
  [255]

  $ cat file2
  2
  $ tglog
  @  6408d34d8180 'commit1'
  $ hg goto --clean --quiet .
  $ hg shelve --list
  default * shelve changes to: commit1 (glob)
  $ hg unshelve
  unshelving change 'default'
  $ cat file2
  2

  $ hg shelve --config abortupdate.after=true --config abortupdate.abort=true
  shelved as default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  interrupted!
  [255]

  $ cat file2
  1
  $ tglog
  @  6408d34d8180 'commit1'
  $ hg shelve --list
  default * shelve changes to: commit1 (glob)
  $ hg log --hidden -r tip -T '{node|short} "{shelvename}" "{desc}"\n'
  f70d92a087e8 "default" "shelve changes to: commit1"
  $ hg unshelve
  unshelving change 'default'
  $ cat file2
  2
