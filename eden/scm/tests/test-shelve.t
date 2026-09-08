
#require no-eden


# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

  $ setconfig devel.segmented-changelog-rev-compat=true
  $ setconfig checkout.use-rust=true

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
  $ newclientrepo obsrepo

  $ mkdir a b
  $ echo a > a/a
  $ echo b > b/b
  $ echo c > c
  $ echo d > d
  $ echo x > x
  $ sl addremove -q
  $ sl shelve
  shelved as default
  0 files updated, 0 files merged, 5 files removed, 0 files unresolved
  $ sl shelve --list
  default * (changes in empty repository) (glob)
  $ sl revert --all
  $ sl unshelve
  unshelving change 'default'
  $ sl diff
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
  $ sl ci -qm 'initial commit'
  $ sl shelve
  nothing changed
  [1]

# Make sure shelve files were backed up

  $ ls .sl/shelve-backup
  default.oshelve
  default.patch

  $ echo n > n
  $ sl add n
  $ sl commit n -m second

# Shelve a change that we will delete later

  $ echo a >> a/a
  $ sl shelve
  shelved as default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

# Set up some more complex shelve changes to shelve

  $ echo a >> a/a
  $ sl mv b b.rename
  moving b/b to b.rename/b
  $ sl cp c c.copy
  $ sl status -C
  M a/a
  A b.rename/b
    b/b
  A c.copy
    c
  R b/b

# The common case - no options or filenames

  $ sl shelve
  shelved as default-01
  2 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ sl status -C

# Ensure that our shelved changes exist

  $ sl shelve -l
  default-01 * shelve changes to: second (glob)
  default * shelve changes to: second (glob)
  $ sl shelve -l -p default
  default * shelve changes to: second (glob)
  
  diff --git a/a/a b/a/a
  --- a/a/a
  +++ b/a/a
  @@ -1,1 +1,2 @@
   a
  +a

  $ sl shelve --list --addremove
  abort: options '--list' and '--addremove' may not be used together
  [255]

# Delete our older shelved change

  $ sl shelve -d default

# Ensure shelve backups aren't overwritten

  $ ls .sl/shelve-backup/
  default-1.oshelve
  default-1.patch
  default.oshelve
  default.patch

# Local edits should not prevent a shelved change from applying

  $ printf 'z\na\n' > a/a
  $ sl unshelve --keep
  unshelving change 'default-01'
  temporarily committing pending changes (restore with 'sl unshelve --abort')
  rebasing shelved changes
  rebasing * "shelve changes to: second" (glob)
  merging a/a

  $ sl revert --all -q
  $ rm a/a.orig b.rename/b c.copy

# Apply it and make sure our state is as expected
# (this also tests that same timestamp prevents backups from being
# removed, even though there are more than 'maxbackups' backups)

  $ test -f .sl/shelve-backup/default.patch
  $ test -f .sl/shelve-backup/default-1.patch

  $ touch -t 200001010000 .sl/shelve-backup/default.patch
  $ touch -t 200001010000 .sl/shelve-backup/default-1.patch

  $ sl unshelve
  unshelving change 'default-01'
  $ sl status -C
  M a/a
  A b.rename/b
    b/b
  A c.copy
    c
  R b/b
  $ sl shelve -l

# (both of default.oshelve and default-1.oshelve should be still kept,
# because it is difficult to decide actual order of them from same timestamp)

  $ ls .sl/shelve-backup/
  default-01.oshelve
  default-01.patch
  default-1.oshelve
  default-1.patch
  default.oshelve
  default.patch
  $ sl unshelve
  abort: no shelved changes to apply!
  [255]
  $ sl unshelve foo
  abort: shelved change 'foo' not found
  [255]

# Named shelves, specific filenames, and "commit messages" should all work
# (this tests also that editor is invoked, if '--edit' is specified)

  $ sl status -C
  M a/a
  A b.rename/b
    b/b
  A c.copy
    c
  R b/b
  $ HGEDITOR=cat sl shelve -q -n wibble -m wat -e a
  wat
  
  
  SL: Enter commit message.  Lines beginning with 'SL:' are removed.
  SL: Leave message empty to abort commit.
  SL: --
  SL: user: test
  SL: changed a/a

# Expect "a" to no longer be present, but status otherwise unchanged

  $ sl status -C
  A b.rename/b
    b/b
  A c.copy
    c
  R b/b
  $ sl shelve -l --stat
  wibble * wat (glob)
   a/a |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)

# And now "a/a" should reappear

  $ cd a
  $ sl unshelve -q wibble
  $ cd ..
  $ sl status -C
  M a/a
  A b.rename/b
    b/b
  A c.copy
    c
  R b/b

# Ensure old shelve backups are being deleted automatically

  $ ls .sl/shelve-backup/
  default-01.oshelve
  default-01.patch
  wibble.oshelve
  wibble.patch

# Cause unshelving to result in a merge with 'a' conflicting

  $ sl shelve -q
  $ echo 'c' >> a/a
  $ sl commit -m second
  $ sl tip --template '{files}\n'
  a/a

# Add an unrelated change that should be preserved

  $ mkdir foo
  $ echo foo > foo/foo
  $ sl add foo/foo

# Force a conflicted merge to occur

  $ sl unshelve
  unshelving change 'default'
  temporarily committing pending changes (restore with 'sl unshelve --abort')
  rebasing shelved changes
  rebasing * "shelve changes to: second" (glob)
  merging a/a
  warning: 1 conflicts while merging a/a! (edit, then use 'sl resolve --mark')
  unresolved conflicts (see 'sl resolve', then 'sl unshelve --continue')
  [1]

# Ensure that we have a merge with unresolved conflicts

  $ sl heads -q --template '{rev}\n'
  11
  4
  $ sl parents -q --template '{rev}\n'
  11
  4
  $ sl status
  M a/a
  M b.rename/b
  M c.copy
  R b/b
  ? a/a.orig
  $ sl diff
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
  $ sl resolve -l
  U a/a

  $ sl shelve
  abort: unshelve in progress
  (use 'sl unshelve --continue' to continue or
       'sl unshelve --abort' to abort)
  [255]

# Abort the unshelve and be happy

  $ sl status
  M a/a
  M b.rename/b
  M c.copy
  R b/b
  ? a/a.orig
  $ sl unshelve -a
  rebase aborted
  unshelve of 'default' aborted
  $ sl heads -q
  c2e78cacc5ac
  $ sl parents -T '{node|short}\n'
  c2e78cacc5ac
  $ sl resolve -l
  $ sl status
  A foo/foo
  ? a/a.orig

# Try to continue with no unshelve underway

  $ sl unshelve -c
  abort: no unshelve in progress
  [255]
  $ sl status
  A foo/foo
  ? a/a.orig

# Redo the unshelve to get a conflict

  $ sl unshelve -q
  warning: 1 conflicts while merging a/a! (edit, then use 'sl resolve --mark')
  unresolved conflicts (see 'sl resolve', then 'sl unshelve --continue')
  [1]

# Attempt to continue

  $ sl unshelve -c
  abort: unresolved conflicts, can't continue
  (see 'sl resolve', then 'sl unshelve --continue')
  [255]
  $ sl revert -r . a/a
  $ sl resolve -m a/a
  (no more unresolved files)
  continue: sl unshelve --continue
  $ sl commit -m 'commit while unshelve in progress'
  abort: unshelve in progress
  (use 'sl unshelve --continue' to continue or
       'sl unshelve --abort' to abort)
  [255]
  $ sl graft --continue
  abort: no graft in progress
  (continue: sl unshelve --continue)
  [255]
  $ sl unshelve -c --trace
  rebasing * "shelve changes to: second" (glob)
  unshelve of 'default' complete

# Ensure the repo is as we hope

  $ sl parents -T '{node|short}\n'
  c2e78cacc5ac
  $ sl heads -q
  201e9c39b40b
  $ sl status -C
  A b.rename/b
    b/b
  A c.copy
    c
  A foo/foo
  R b/b
  ? a/a.orig

# There should be no shelves left

  $ sl shelve -l

#if execbit
# Ensure that metadata-only changes are shelved

  $ chmod +x a/a

  $ sl shelve -q -n execbit a/a
  $ sl status a/a
  $ sl unshelve -q execbit
  $ sl status a/a
  M a/a
  $ sl revert a/a
#endif

#if symlink
# Ensure symlinks are properly handled
  $ rm a/a
  $ ln -s foo a/a
  $ sl shelve -q -n symlink a/a
  $ sl status a/a
  $ sl unshelve -q symlink
  $ sl status a/a
  M a/a
  $ sl revert a/a
#endif

# Set up another conflict between a commit and a shelved change

  $ sl revert -q -C -a
  $ rm a/a.orig b.rename/b c.copy
  $ echo a >> a/a
  $ sl shelve -q
  $ echo x >> a/a
  $ sl ci -m 'create conflict'
  $ sl add foo/foo

# If we resolve a conflict while unshelving, the unshelve should succeed

  $ sl unshelve --tool ':merge-other' --keep
  unshelving change 'default'
  temporarily committing pending changes (restore with 'sl unshelve --abort')
  rebasing shelved changes
  rebasing .* "shelve changes to: second" (re)
  merging a/a
  $ sl shelve -l
  default * shelve changes to: second (glob)
  $ sl status
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
  $ HGMERGE=true sl unshelve
  unshelving change 'default'
  temporarily committing pending changes (restore with 'sl unshelve --abort')
  rebasing shelved changes
  rebasing * "shelve changes to: second" (glob)
  merging a/a
  note: not rebasing *, its destination (rebasing onto) commit already has all its changes (glob)
  $ sl shelve -l
  $ sl status
  M a/a
  A foo/foo
  $ cat a/a
  a
  c
  a

# Test keep and cleanup

  $ sl shelve
  shelved as default
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ sl shelve --list
  default * shelve changes to: create conflict (glob)
  $ sl unshelve -k
  unshelving change 'default'
  $ sl shelve --list
  default * shelve changes to: create conflict (glob)
  $ sl shelve --cleanup
  $ sl shelve --list

# Test bookmarks

  $ sl bookmark test
  $ sl bookmark
  * test                      * (glob)
  $ sl shelve
  shelved as test
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ sl bookmark
  * test                      * (glob)
  $ sl unshelve
  unshelving change 'test'
  $ sl bookmark
  * test                      * (glob)

# Shelve should still work even if mq is disabled

  $ sl --config 'extensions.mq=!' shelve
  shelved as test
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ sl --config 'extensions.mq=!' shelve --list
  test * shelve changes to: create conflict (glob)
  $ sl bookmark
  * test                      * (glob)
  $ sl --config 'extensions.mq=!' unshelve
  unshelving change 'test'
  $ sl bookmark
  * test                      * (glob)
  $ cd ..

# Shelve should leave dirstate clean (issue4055)

  $ newclientrepo
  $ printf 'x\ny\n' > x
  $ echo z > z
  $ sl commit -Aqm xy
  $ echo z >> x
  $ sl commit -Aqm z
  $ sl up 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ printf 'a\nx\ny\nz\n' > x
  $ sl commit -Aqm xyz
  $ echo c >> z
  $ sl shelve
  shelved as default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ sl rebase -d 1 --config 'extensions.rebase='
  rebasing 323bfa07f744 "xyz"
  merging x
  $ sl unshelve
  unshelving change 'default'
  rebasing shelved changes
  rebasing a2281b51947d "shelve changes to: xyz"
  $ sl status
  M z
  $ cd ..

# Shelve should only unshelve pending changes (issue4068)

  $ newclientrepo
  $ touch a
  $ sl ci -Aqm a
  $ touch b
  $ sl ci -Aqm b
  $ sl up -q 0
  $ touch c
  $ sl ci -Aqm c
  $ touch d
  $ sl add d
  $ sl shelve
  shelved as default
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ sl up -q 1
  $ sl unshelve
  unshelving change 'default'
  rebasing shelved changes
  rebasing 7eac9d98447f "shelve changes to: c"
  $ sl status
  A d

# Unshelve should work on an ancestor of the original commit

  $ sl shelve
  shelved as default
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ sl up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ sl unshelve
  unshelving change 'default'
  rebasing shelved changes
  rebasing 325b64d70042 "shelve changes to: b"
  $ sl status
  A d

# Unshelve should leave unknown files alone (issue4113)

  $ echo e > e
  $ sl shelve
  shelved as default
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ sl status
  ? e
  $ sl unshelve
  unshelving change 'default'
  $ sl status
  A d
  ? e
  $ cat e
  e

# 139. Unshelve should keep a copy of unknown files

  $ sl add e
  $ sl shelve
  shelved as default
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ echo z > e
  $ sl unshelve
  unshelving change 'default'
  $ cat e
  e
  $ cat e.orig
  z

# 140. Unshelve and conflicts with tracked and untracked files
#  preparing:

  $ rm 'e.orig'
  $ sl ci -qm 'commit stuff'
  $ sl debugmakepublic 'null:'

#  no other changes - no merge:

  $ echo f > f
  $ sl add f
  $ sl shelve
  shelved as default
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo g > f
  $ sl unshelve
  unshelving change 'default'
  $ sl st
  A f
  ? f.orig
  $ cat f
  f
  $ cat f.orig
  g

#  other uncommitted changes - merge:

  $ sl st
  A f
  ? f.orig
  $ sl shelve
  shelved as default
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ sl log -G --template '{rev}  {desc|firstline}  {author}'
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
  $ sl unshelve --date '1073741824 0'
  unshelving change 'default'
  temporarily committing pending changes (restore with 'sl unshelve --abort')
  rebasing shelved changes
  rebasing a0cc43106cdd "shelve changes to: commit stuff"
  merging f
  warning: 1 conflicts while merging f! (edit, then use 'sl resolve --mark')
  unresolved conflicts (see 'sl resolve', then 'sl unshelve --continue')
  [1]
  $ sl parents -T '{desc|firstline}\n'
  pending changes temporary commit
  shelve changes to: commit stuff

  $ sl st
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
  $ sl unshelve --abort -t false
  tool option will be ignored
  rebase aborted
  unshelve of 'default' aborted
  $ sl st
  M a
  ? f.orig
  $ cat f.orig
  g
  $ sl unshelve
  unshelving change 'default'
  temporarily committing pending changes (restore with 'sl unshelve --abort')
  rebasing shelved changes
  rebasing a0cc43106cdd "shelve changes to: commit stuff"
  $ sl st
  M a
  A f
  ? f.orig

#  other committed changes - merge:

  $ sl shelve f
  shelved as default
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ sl ci a -m 'intermediate other change'
  $ mv f.orig f
  $ sl unshelve
  unshelving change 'default'
  rebasing shelved changes
  rebasing a0cc43106cdd "shelve changes to: commit stuff"
  merging f
  warning: 1 conflicts while merging f! (edit, then use 'sl resolve --mark')
  unresolved conflicts (see 'sl resolve', then 'sl unshelve --continue')
  [1]
  $ sl st
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
  $ sl unshelve --abort
  rebase aborted
  unshelve of 'default' aborted
  $ sl st
  ? f.orig
  $ cat f.orig
  g
  $ sl shelve --delete default

# Recreate some conflict again

  $ cd ../obsrepo
  $ sl up -C -r 'test^'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark test)
  $ echo y >> a/a
  $ sl shelve
  shelved as default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ sl up test
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark test)
  $ sl bookmark
  * test                      * (glob)
  $ sl unshelve
  unshelving change 'default'
  rebasing shelved changes
  rebasing * "shelve changes to: second" (glob)
  merging a/a
  warning: 1 conflicts while merging a/a! (edit, then use 'sl resolve --mark')
  unresolved conflicts (see 'sl resolve', then 'sl unshelve --continue')
  [1]
  $ sl bookmark
     test * (glob)

# Test that resolving all conflicts in one direction (so that the rebase
# is a no-op), works (issue4398)

  $ sl revert -a -r .
  reverting a/a
  $ sl resolve -m a/a
  (no more unresolved files)
  continue: sl unshelve --continue
  $ sl unshelve -c
  rebasing * "shelve changes to: second" (glob)
  note: not rebasing *, its destination (rebasing onto) commit already has all its changes (glob)
  unshelve of 'default' complete
  $ sl bookmark
  * test                      * (glob)
  $ sl diff
  $ sl status
  ? a/a.orig
  ? foo/foo

  $ sl shelve --delete --stat
  abort: options '--delete' and '--stat' may not be used together
  [255]
  $ sl shelve --delete --name NAME
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
  $ sl st
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
  $ sl shelve --interactive --config 'ui.interactive=false'
  abort: running non-interactively
  [255]
  $ sl shelve --interactive << 'EOS'
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
  $ sl st
  M a/a
  ? foo/foo
  $ sl bookmark
  * test                      * (glob)
  $ sl log -r . -T '{desc|firstline}\n'
  create conflict
  $ sl unshelve
  unshelving change 'test'
  temporarily committing pending changes (restore with 'sl unshelve --abort')
  rebasing shelved changes
  rebasing * "shelve changes to: create conflict" (glob)
  merging a/a
  $ sl bookmark
  * test                      * (glob)
  $ sl log -r . -T '{desc|firstline}\n'
  create conflict
  $ cat a/a
  a
  a
  c
  x
  x

# Shelve --patch and shelve --stat should work with a single valid shelfname

  $ sl up --clean .
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark test)
  $ sl shelve --list
  $ echo 'patch a' > shelf-patch-a
  $ sl add shelf-patch-a
  $ sl shelve
  shelved as default
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo 'patch b' > shelf-patch-b
  $ sl add shelf-patch-b
  $ sl shelve
  shelved as default-01
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ sl shelve --patch default default-01
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
  $ sl shelve --stat default default-01
  default-01 * shelve changes to: create conflict (glob)
   shelf-patch-b |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  default * shelve changes to: create conflict (glob)
   shelf-patch-a |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  $ sl shelve --patch default
  default * shelve changes to: create conflict (glob)
  
  diff --git a/shelf-patch-a b/shelf-patch-a
  new file mode 100644
  --- /dev/null
  +++ b/shelf-patch-a
  @@ -0,0 +1,1 @@
  +patch a

# No-argument --patch should also work

  $ sl shelve --patch
  default-01      (*)*shelve changes to: create conflict (glob)
  
  diff --git a/shelf-patch-b b/shelf-patch-b
  new file mode 100644
  --- /dev/null
  +++ b/shelf-patch-b
  @@ -0,0 +1,1 @@
  +patch b
  $ sl shelve --stat default
  default * shelve changes to: create conflict (glob)
   shelf-patch-a |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  $ sl shelve --patch nonexistentshelf
  abort: cannot find shelf nonexistentshelf
  [255]
  $ sl shelve --stat nonexistentshelf
  abort: cannot find shelf nonexistentshelf
  [255]

# Test .orig files go where the user wants them to
# ---------------------------------------------------------------

  $ newclientrepo obssh-salvage
  $ echo content > root
  $ sl commit -A -m root -q
  $ echo '' > root
  $ sl shelve -q
  $ echo contADDent > root
  $ sl unshelve -q --config 'ui.origbackuppath=@DOTDIR@/origbackups'
  warning: 1 conflicts while merging root! (edit, then use 'sl resolve --mark')
  unresolved conflicts (see 'sl resolve', then 'sl unshelve --continue')
  [1]
  $ ls .sl/origbackups
  root
  $ rm -rf .sl/origbackups

# Test Abort unshelve always gets user out of the unshelved state
# ---------------------------------------------------------------
# Wreak havoc on the unshelve process

  $ rm .sl/unshelverebasestate

  $ sl unshelve --abort
  unshelve of 'default' aborted
  abort: $ENOENT$: $TESTTMP/obssh-salvage/.sl/unshelverebasestate
  [255]

# Can the user leave the current state?

  $ sl up -C .
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

# Try again but with a corrupted shelve state file

  $ sl up -r 0 -q
  $ echo '' > root
  $ sl shelve -q
  $ echo contADDent > root
  $ sl unshelve -q
  warning: 1 conflicts while merging root! (edit, then use 'sl resolve --mark')
  unresolved conflicts (see 'sl resolve', then 'sl unshelve --continue')
  [1]

  $ sed 's/ae8c668541e8/123456789012/' .sl/shelvedstate > ../corrupt-shelvedstate
  $ mv ../corrupt-shelvedstate .sl/shelvedstate

  $ sl unshelve --abort
  could not read shelved state file, your working copy may be in an unexpected state
  please update to some commit
  $ sl up -C .
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
  $ sl bookmarks -R obsrepo
     test * (glob)
  $ sl share -B obsrepo obsshare
  updating working directory
  7 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd obsshare

  $ sl bookmarks
     test                      * (glob)
  $ sl bookmarks foo
  $ sl bookmarks
   * foo                       * (glob)
     test                      * (glob)
  $ echo x >> x
  $ sl shelve
  shelved as foo
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ sl bookmarks
   * foo                       * (glob)
     test                      * (glob)

  $ sl unshelve
  unshelving change 'foo'
  $ sl bookmarks
   * foo                       * (glob)
     test                      * (glob)

  $ cd ..

# Shelve and unshelve unknown files. For the purposes of unshelve, a shelved
# unknown file is the same as a shelved added file, except that it will be in
# unknown state after unshelve if and only if it was either absent or unknown
# before the unshelve operation.

  $ newclientrepo

# The simplest case is if I simply have an unknown file that I shelve and unshelve

  $ echo unknown > unknown
  $ sl status
  ? unknown
  $ sl shelve --unknown
  shelved as default
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ sl status
  $ sl unshelve
  unshelving change 'default'
  $ sl status
  ? unknown
  $ rm unknown

# If I shelve, add the file, and unshelve, does it stay added?

  $ echo unknown > unknown
  $ sl shelve -u
  shelved as default
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ sl status
  $ touch unknown
  $ sl add unknown
  $ sl status
  A unknown
  $ sl unshelve
  unshelving change 'default'
  temporarily committing pending changes (restore with 'sl unshelve --abort')
  rebasing shelved changes
  rebasing c850bce25d9f "(changes in empty repository)"
  merging unknown
  $ sl status
  A unknown
  $ sl forget unknown
  $ rm unknown

# And if I shelve, commit, then unshelve, does it become modified?

  $ echo unknown > unknown
  $ sl shelve -u
  shelved as default
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ sl status
  $ touch unknown
  $ sl add unknown
  $ sl commit -qm 'Add unknown'
  $ sl status
  $ sl unshelve
  unshelving change 'default'
  rebasing shelved changes
  rebasing c850bce25d9f "(changes in empty repository)"
  merging unknown
  $ sl status
  M unknown
  $ sl remove --force unknown
  $ sl commit -qm 'Remove unknown'
  $ cd ..

# And if I shelve an unknown file and unshelve on a different commit, it should
# stay unknown. The file is absent before the unshelve, so by the rule above it
# must come back in unknown state even though unshelve has to rebase.

  $ newclientrepo unknownrebase
  $ drawdag <<'EOS'
  > B
  > |
  > A
  > EOS
  $ sl goto -q $B
  $ echo unknown > unknown
  $ sl status
  ? unknown
  $ sl shelve --unknown
  shelved as default
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ sl status

Moving to another commit sends unshelve down its rebase path:

  $ sl goto -q $A
  $ sl unshelve
  unshelving change 'default'
  rebasing shelved changes
  rebasing 50d1e3771ebc "shelve changes to: B"
  $ sl status
  ? unknown
  $ cd ..

# Prepare unshelve with a corrupted shelvedstate

  $ newclientrepo
  $ echo text1 > file
  $ sl add file
  $ sl shelve
  shelved as default
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo text2 > file
  $ sl ci -Am text1
  adding file
  $ sl unshelve
  unshelving change 'default'
  rebasing shelved changes
  rebasing a6a994ce5ac2 "(changes in empty repository)"
  merging file
  warning: 1 conflicts while merging file! (edit, then use 'sl resolve --mark')
  unresolved conflicts (see 'sl resolve', then 'sl unshelve --continue')
  [1]
  $ echo somethingsomething > .sl/shelvedstate

# Unshelve --continue fails with appropriate message if shelvedstate is corrupted

  $ sl continue
  abort: corrupted shelved state file
  (please run sl unshelve --abort to abort unshelve operation)
  [255]

# Unshelve --abort works with a corrupted shelvedstate

  $ sl unshelve --abort
  could not read shelved state file, your working copy may be in an unexpected state
  please update to some commit

# Unshelve --abort fails with appropriate message if there's no unshelve in
# progress

  $ sl unshelve --abort
  abort: no unshelve in progress
  [255]
  $ cd ..

# Unshelve respects --keep even if user intervention is needed

  $ newclientrepo
  $ echo 1 > file
  $ sl ci -Am 1
  adding file
  $ echo 2 >> file
  $ sl shelve
  shelved as default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo 3 > file
  $ sl ci -Am 13
  $ sl shelve --list
  default * shelve changes to: 1 (glob)
  $ sl unshelve --keep
  unshelving change 'default'
  rebasing shelved changes
  rebasing 49351a7ca591 "shelve changes to: 1"
  merging file
  warning: 1 conflicts while merging file! (edit, then use 'sl resolve --mark')
  unresolved conflicts (see 'sl resolve', then 'sl unshelve --continue')
  [1]
  $ sl resolve --mark file
  (no more unresolved files)
  continue: sl unshelve --continue
  $ sl unshelve --continue
  rebasing 49351a7ca591 "shelve changes to: 1"
  unshelve of 'default' complete
  $ sl shelve --list
  default * shelve changes to: 1 (glob)
  $ cd ..

# Unshelving a stripped commit aborts with an explanatory message

  $ newclientrepo
  $ echo 1 > file
  $ sl ci -Am 1
  adding file
  $ echo 2 >> file
  $ sl shelve
  shelved as default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ sl debugstrip -r 1 --config 'experimental.evolution=!' --hidden
  $ sl unshelve
  unshelving change 'default'
  abort: shelved node 49351a7ca59142b32064896a48f50bdecccf8ea0 not found in repo
  If you think this shelve should exist, try running 'sl import --no-commit .sl/shelved/default.patch' from the root of the repository.
  [255]
  $ cd ..

# Test revsetpredicate 'shelved'
# For this test enabled shelve extension is enough, and it is enabled at the top of the file

  $ newclientrepo

  $ testshelvedcount() {
  >   local count=$(sl log -r "shelved()" -T "{node}\n" | wc -l)
  >   [ $count -eq $1 ]
  > }

  $ touch file1
  $ touch file2
  $ touch file3
  $ sl addremove
  adding file1
  adding file2
  adding file3
  $ sl commit -m 'Add test files'
  $ echo 1 >> file1
  $ sl shelve
  shelved as default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ testshelvedcount 1
  $ echo 2 >> file2
  $ sl shelve
  shelved as default-01
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ testshelvedcount 2
  $ echo 3 >> file3
  $ sl shelve
  shelved as default-02
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ testshelvedcount 3
  $ sl log --hidden -r 'shelved()' --template '{node|short} {shelvename}\n'
  d7a61836580c default
  9dcce8f0ff7d default-01
  225e1bca0190 default-02
  $ sl unshelve > /dev/null
  $ testshelvedcount 2
  $ sl unshelve > /dev/null
  $ testshelvedcount 1
  $ sl unshelve > /dev/null
  $ testshelvedcount 0
  $ cd ..

# Test interrupted shelve - this should not lose work

  $ newclientrepo
  $ echo 1 > file1
  $ echo 1 > file2
  $ sl commit -Aqm commit1
  $ echo 2 > file2

  $ cat file2
  2
  $ tglog
  @  6408d34d8180 'commit1'

  $ cat >> $TESTTMP/abortupdate.py << 'EOF'
  > from sapling import extensions, hg, error
  > def update(orig, repo, *args, **kwargs):
  >     if not repo.ui.configbool("abortupdate", "abort"):
  >         return orig(repo, *args, **kwargs)
  >     if repo.ui.configbool("abortupdate", "after"):
  >         orig(repo, *args, **kwargs)
  >     raise error.Abort("update aborted!")
  > def extsetup(ui):
  >     extensions.wrapfunction(hg, "update", update)
  > EOF

  $ setconfig extensions.abortcreatemarkers="$TESTTMP/abortupdate.py"
  $ sl shelve --config abortupdate.abort=true
  shelved as default
  abort: update aborted!
  [255]

  $ cat file2
  2
  $ tglog
  @  6408d34d8180 'commit1'
  $ sl goto --clean --quiet .
  $ sl shelve --list
  default * shelve changes to: commit1 (glob)
  $ sl unshelve
  unshelving change 'default'
  $ cat file2
  2

  $ sl shelve --config abortupdate.after=true --config abortupdate.abort=true
  shelved as default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  abort: update aborted!
  [255]

  $ cat file2
  1
  $ tglog
  @  6408d34d8180 'commit1'
  $ sl shelve --list
  default * shelve changes to: commit1 (glob)
  $ sl log --hidden -r tip -T '{node|short} "{shelvename}" "{desc}"\n'
  f70d92a087e8 "default" "shelve changes to: commit1"
  $ sl unshelve
  unshelving change 'default'
  $ cat file2
  2

# Test unshelve new added files in a sub-directory (non-reporoot)

  $ newclientrepo
  $ mkdir foo && cd foo
  $ touch a
  $ sl ci -Aqm a
  $ touch b
  $ sl add b
  $ sl shelve
  shelved as default
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ touch b
  $ sl st
  ? b
  $ sl unshelve
  unshelving change 'default'
  $ sl st
  A b
  ? b.orig
