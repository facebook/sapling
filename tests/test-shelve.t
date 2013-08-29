  $ echo "[extensions]" >> $HGRCPATH
  $ echo "shelve=" >> $HGRCPATH
  $ echo "[defaults]" >> $HGRCPATH
  $ echo "diff = --nodates --git" >> $HGRCPATH

  $ hg init repo
  $ cd repo
  $ mkdir a b
  $ echo a > a/a
  $ echo b > b/b
  $ echo c > c
  $ echo d > d
  $ echo x > x
  $ hg addremove -q

shelving in an empty repo should be possible

  $ hg shelve
  (empty repository)
  shelved as default
  0 files updated, 0 files merged, 5 files removed, 0 files unresolved

  $ hg unshelve
  unshelving change 'default'
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 5 changes to 5 files
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg commit -q -m 'initial commit'

  $ hg shelve
  nothing changed
  [1]

create another commit

  $ echo n > n
  $ hg add n
  $ hg commit n -m second

shelve a change that we will delete later

  $ echo a >> a/a
  $ hg shelve
  shelved from default (bb4fec6d): second
  shelved as default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

set up some more complex changes to shelve

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

prevent some foot-shooting

  $ hg shelve -n foo/bar
  abort: shelved change names may not contain slashes
  [255]
  $ hg shelve -n .baz
  abort: shelved change names may not start with '.'
  [255]

the common case - no options or filenames

  $ hg shelve
  shelved from default (bb4fec6d): second
  shelved as default-01
  2 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg status -C

ensure that our shelved changes exist

  $ hg shelve -l
  default-01      [*]    shelved from default (bb4fec6d): second (glob)
  default         [*]    shelved from default (bb4fec6d): second (glob)

  $ hg shelve -l -p default
  default         [*]    shelved from default (bb4fec6d): second (glob)
  
  diff --git a/a/a b/a/a
  --- a/a/a
  +++ b/a/a
  @@ -1,1 +1,2 @@
   a
  +a

delete our older shelved change

  $ hg shelve -d default

local edits should prevent a shelved change from applying

  $ echo e>>a/a
  $ hg unshelve
  unshelving change 'default-01'
  the following shelved files have been modified:
    a/a
  you must commit, revert, or shelve your changes before you can proceed
  abort: cannot unshelve due to local changes
  
  [255]

  $ hg revert -C a/a

apply it and make sure our state is as expected

  $ hg unshelve
  unshelving change 'default-01'
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 3 changes to 8 files
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg status -C
  M a/a
  A b.rename/b
    b/b
  A c.copy
    c
  R b/b
  $ hg shelve -l

  $ hg unshelve
  abort: no shelved changes to apply!
  [255]
  $ hg unshelve foo
  abort: shelved change 'foo' not found
  [255]

named shelves, specific filenames, and "commit messages" should all work

  $ hg status -C
  M a/a
  A b.rename/b
    b/b
  A c.copy
    c
  R b/b
  $ hg shelve -q -n wibble -m wat a

expect "a" to no longer be present, but status otherwise unchanged

  $ hg status -C
  A b.rename/b
    b/b
  A c.copy
    c
  R b/b
  $ hg shelve -l --stat
  wibble          [*]    wat (glob)
   a/a |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)

and now "a/a" should reappear

  $ hg unshelve -q wibble
  $ hg status -C
  M a/a
  A b.rename/b
    b/b
  A c.copy
    c
  R b/b

cause unshelving to result in a merge with 'a' conflicting

  $ hg shelve -q
  $ echo c>>a/a
  $ hg commit -m second
  $ hg tip --template '{files}\n'
  a/a

add an unrelated change that should be preserved

  $ mkdir foo
  $ echo foo > foo/foo
  $ hg add foo/foo

force a conflicted merge to occur

  $ hg unshelve
  unshelving change 'default'
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 3 changes to 8 files (+1 heads)
  merging a/a
  warning: conflicts during merge.
  merging a/a incomplete! (edit conflicts, then use 'hg resolve --mark')
  2 files updated, 0 files merged, 1 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  unresolved conflicts (see 'hg resolve', then 'hg unshelve --continue')
  [1]

ensure that we have a merge with unresolved conflicts

  $ hg heads -q
  3:7ec047b69dc0
  2:ceefc37abe1e
  $ hg parents -q
  2:ceefc37abe1e
  3:7ec047b69dc0
  $ hg status
  M a/a
  M b.rename/b
  M c.copy
  A foo/foo
  R b/b
  ? a/a.orig
  $ hg diff
  diff --git a/a/a b/a/a
  --- a/a/a
  +++ b/a/a
  @@ -1,2 +1,6 @@
   a
  +<<<<<<< local
   c
  +=======
  +a
  +>>>>>>> other
  diff --git a/b.rename/b b/b.rename/b
  --- /dev/null
  +++ b/b.rename/b
  @@ -0,0 +1,1 @@
  +b
  diff --git a/b/b b/b/b
  deleted file mode 100644
  --- a/b/b
  +++ /dev/null
  @@ -1,1 +0,0 @@
  -b
  diff --git a/c.copy b/c.copy
  --- /dev/null
  +++ b/c.copy
  @@ -0,0 +1,1 @@
  +c
  diff --git a/foo/foo b/foo/foo
  new file mode 100644
  --- /dev/null
  +++ b/foo/foo
  @@ -0,0 +1,1 @@
  +foo
  $ hg resolve -l
  U a/a

  $ hg shelve
  abort: unshelve already in progress
  (use 'hg unshelve --continue' or 'hg unshelve --abort')
  [255]

abort the unshelve and be happy

  $ hg status
  M a/a
  M b.rename/b
  M c.copy
  A foo/foo
  R b/b
  ? a/a.orig
  $ hg unshelve -a
  unshelve of 'default' aborted
  $ hg heads -q
  2:ceefc37abe1e
  $ hg parents
  changeset:   2:ceefc37abe1e
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     second
  
  $ hg resolve -l
  $ hg status
  A foo/foo
  ? a/a.orig

try to continue with no unshelve underway

  $ hg unshelve -c
  abort: no unshelve operation underway
  [255]
  $ hg status
  A foo/foo
  ? a/a.orig

redo the unshelve to get a conflict

  $ hg unshelve -q
  warning: conflicts during merge.
  merging a/a incomplete! (edit conflicts, then use 'hg resolve --mark')
  unresolved conflicts (see 'hg resolve', then 'hg unshelve --continue')
  [1]

attempt to continue

  $ hg unshelve -c
  abort: unresolved conflicts, can't continue
  (see 'hg resolve', then 'hg unshelve --continue')
  [255]

  $ hg revert -r . a/a
  $ hg resolve -m a/a

  $ hg unshelve -c
  unshelve of 'default' complete

ensure the repo is as we hope

  $ hg parents
  changeset:   2:ceefc37abe1e
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     second
  
  $ hg heads -q
  2:ceefc37abe1e

  $ hg status -C
  M a/a
  M b.rename/b
    b/b
  M c.copy
    c
  A foo/foo
  R b/b
  ? a/a.orig

there should be no shelves left

  $ hg shelve -l

  $ hg commit -m whee a/a

#if execbit

ensure that metadata-only changes are shelved

  $ chmod +x a/a
  $ hg shelve -q -n execbit a/a
  $ hg status a/a
  $ hg unshelve -q execbit
  $ hg status a/a
  M a/a
  $ hg revert a/a

#endif

#if symlink

  $ rm a/a
  $ ln -s foo a/a
  $ hg shelve -q -n symlink a/a
  $ hg status a/a
  $ hg unshelve -q symlink
  $ hg status a/a
  M a/a
  $ hg revert a/a

#endif

set up another conflict between a commit and a shelved change

  $ hg revert -q -C -a
  $ echo a >> a/a
  $ hg shelve -q
  $ echo x >> a/a
  $ hg ci -m 'create conflict'
  $ hg add foo/foo

if we resolve a conflict while unshelving, the unshelve should succeed

  $ HGMERGE=true hg unshelve
  unshelving change 'default'
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 6 files (+1 heads)
  merging a/a
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  $ hg parents -q
  4:be7e79683c99
  $ hg shelve -l
  $ hg status
  M a/a
  A foo/foo
  $ cat a/a
  a
  c
  x

test keep and cleanup

  $ hg shelve
  shelved from default (be7e7968): create conflict
  shelved as default
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg shelve --list
  default         [*]    shelved from default (be7e7968): create conflict (glob)
  $ hg unshelve --keep
  unshelving change 'default'
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 7 files
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg shelve --list
  default         [*]    shelved from default (be7e7968): create conflict (glob)
  $ hg shelve --cleanup
  $ hg shelve --list
