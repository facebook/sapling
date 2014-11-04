  $ cat <<EOF >> $HGRCPATH
  > [extensions]
  > mq =
  > shelve =
  > [defaults]
  > diff = --nodates --git
  > qnew = --date '0 0'
  > EOF

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
(this tests also that editor is not invoked, if '--edit' is not
specified)

  $ HGEDITOR=cat hg shelve
  shelved as default
  0 files updated, 0 files merged, 5 files removed, 0 files unresolved

  $ hg unshelve
  unshelving change 'default'

  $ hg commit -q -m 'initial commit'

  $ hg shelve
  nothing changed
  [1]

create an mq patch - shelving should work fine with a patch applied

  $ echo n > n
  $ hg add n
  $ hg commit n -m second
  $ hg qnew second.patch

shelve a change that we will delete later

  $ echo a >> a/a
  $ hg shelve
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
  shelved as default-01
  2 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg status -C

ensure that our shelved changes exist

  $ hg shelve -l
  default-01      (*)    changes to '[mq]: second.patch' (glob)
  default         (*)    changes to '[mq]: second.patch' (glob)

  $ hg shelve -l -p default
  default         (*)    changes to '[mq]: second.patch' (glob)
  
  diff --git a/a/a b/a/a
  --- a/a/a
  +++ b/a/a
  @@ -1,1 +1,2 @@
   a
  +a

  $ hg shelve --list --addremove
  abort: options '--list' and '--addremove' may not be used together
  [255]

delete our older shelved change

  $ hg shelve -d default
  $ hg qfinish -a -q

local edits should not prevent a shelved change from applying

  $ printf "z\na\n" > a/a
  $ hg unshelve --keep
  unshelving change 'default-01'
  temporarily committing pending changes (restore with 'hg unshelve --abort')
  rebasing shelved changes
  merging a/a

  $ hg revert --all -q
  $ rm a/a.orig b.rename/b c.copy

apply it and make sure our state is as expected

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

  $ hg unshelve
  abort: no shelved changes to apply!
  [255]
  $ hg unshelve foo
  abort: shelved change 'foo' not found
  [255]

named shelves, specific filenames, and "commit messages" should all work
(this tests also that editor is invoked, if '--edit' is specified)

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
  HG: user: shelve@localhost
  HG: branch 'default'
  HG: changed a/a

expect "a" to no longer be present, but status otherwise unchanged

  $ hg status -C
  A b.rename/b
    b/b
  A c.copy
    c
  R b/b
  $ hg shelve -l --stat
  wibble          (*)    wat (glob)
   a/a |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)

and now "a/a" should reappear

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
  temporarily committing pending changes (restore with 'hg unshelve --abort')
  rebasing shelved changes
  merging a/a
  warning: conflicts during merge.
  merging a/a incomplete! (edit conflicts, then use 'hg resolve --mark')
  unresolved conflicts (see 'hg resolve', then 'hg unshelve --continue')
  [1]

ensure that we have a merge with unresolved conflicts

  $ hg heads -q --template '{rev}\n'
  5
  4
  $ hg parents -q --template '{rev}\n'
  4
  5
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
  +<<<<<<< dest:   *  - shelve: pending changes temporary commit (glob)
   c
  +=======
  +a
  +>>>>>>> source: 4702e8911fe0 - shelve: changes to '[mq]: second.patch'
  diff --git a/b/b b/b.rename/b
  rename from b/b
  rename to b.rename/b
  diff --git a/b/b b/b/b
  deleted file mode 100644
  --- a/b/b
  +++ /dev/null
  @@ -1,1 +0,0 @@
  -b
  diff --git a/c b/c.copy
  copy from c
  copy to c.copy
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
  R b/b
  ? a/a.orig
  $ hg unshelve -a
  rebase aborted
  unshelve of 'default' aborted
  $ hg heads -q
  3:2e69b451d1ea
  $ hg parents
  changeset:   3:2e69b451d1ea
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
  (no more unresolved files)

  $ hg commit -m 'commit while unshelve in progress'
  abort: unshelve already in progress
  (use 'hg unshelve --continue' or 'hg unshelve --abort')
  [255]

  $ hg unshelve -c
  unshelve of 'default' complete

ensure the repo is as we hope

  $ hg parents
  changeset:   3:2e69b451d1ea
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     second
  
  $ hg heads -q
  3:2e69b451d1ea

  $ hg status -C
  A b.rename/b
    b/b
  A c.copy
    c
  A foo/foo
  R b/b
  ? a/a.orig

there should be no shelves left

  $ hg shelve -l

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
  $ rm a/a.orig b.rename/b c.copy
  $ echo a >> a/a
  $ hg shelve -q
  $ echo x >> a/a
  $ hg ci -m 'create conflict'
  $ hg add foo/foo

if we resolve a conflict while unshelving, the unshelve should succeed

  $ HGMERGE=true hg unshelve
  unshelving change 'default'
  temporarily committing pending changes (restore with 'hg unshelve --abort')
  rebasing shelved changes
  merging a/a
  $ hg parents -q
  4:33f7f61e6c5e
  $ hg shelve -l
  $ hg status
  A foo/foo
  $ cat a/a
  a
  c
  x

test keep and cleanup

  $ hg shelve
  shelved as default
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg shelve --list
  default         (*)    changes to 'create conflict' (glob)
  $ hg unshelve --keep
  unshelving change 'default'
  $ hg shelve --list
  default         (*)    changes to 'create conflict' (glob)
  $ hg shelve --cleanup
  $ hg shelve --list

  $ hg shelve --cleanup --delete
  abort: options '--cleanup' and '--delete' may not be used together
  [255]
  $ hg shelve --cleanup --patch
  abort: options '--cleanup' and '--patch' may not be used together
  [255]
  $ hg shelve --cleanup --message MESSAGE
  abort: options '--cleanup' and '--message' may not be used together
  [255]

test bookmarks

  $ hg bookmark test
  $ hg bookmark
   * test                      4:33f7f61e6c5e
  $ hg shelve
  shelved as test
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg bookmark
   * test                      4:33f7f61e6c5e
  $ hg unshelve
  unshelving change 'test'
  $ hg bookmark
   * test                      4:33f7f61e6c5e

shelve should still work even if mq is disabled

  $ hg --config extensions.mq=! shelve
  shelved as test
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg --config extensions.mq=! shelve --list
  test            (*)    changes to 'create conflict' (glob)
  $ hg --config extensions.mq=! unshelve
  unshelving change 'test'

shelve should leave dirstate clean (issue4055)

  $ cd ..
  $ hg init shelverebase
  $ cd shelverebase
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
  $ hg rebase -d 1 --config extensions.rebase=
  merging x
  saved backup bundle to $TESTTMP/shelverebase/.hg/strip-backup/323bfa07f744-backup.hg (glob)
  $ hg unshelve
  unshelving change 'default'
  rebasing shelved changes
  $ hg status
  M z

  $ cd ..

shelve should only unshelve pending changes (issue4068)

  $ hg init onlypendingchanges
  $ cd onlypendingchanges
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
  $ hg status
  A d

unshelve should work on an ancestor of the original commit

  $ hg shelve
  shelved as default
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg unshelve
  unshelving change 'default'
  rebasing shelved changes
  $ hg status
  A d

test bug 4073 we need to enable obsolete markers for it

  $ cat >> $HGRCPATH << EOF
  > [experimental]
  > evolution=createmarkers
  > EOF
  $ hg shelve
  shelved as default
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg debugobsolete `hg --debug id -i -r 1`
  $ hg unshelve
  unshelving change 'default'

unshelve should leave unknown files alone (issue4113)

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

unshelve should keep a copy of unknown files

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


unshelve and conflicts with tracked and untracked files

 preparing:

  $ rm *.orig
  $ hg ci -qm 'commit stuff'
  $ hg phase -p null:

 no other changes - no merge:

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

 other uncommitted changes - merge:

  $ hg st
  A f
  ? f.orig
  $ hg shelve
  shelved as default
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg log -G --template '{rev}  {desc|firstline}  {author}' -R bundle://.hg/shelved/default.hg -r 'bundle()'
  o  4  changes to 'commit stuff'  shelve@localhost
  |
  $ hg log -G --template '{rev}  {desc|firstline}  {author}'
  @  3  commit stuff  test
  |
  | o  2  c  test
  |/
  o  0  a  test
  
  $ mv f.orig f
  $ echo 1 > a
  $ hg unshelve --date '1073741824 0'
  unshelving change 'default'
  temporarily committing pending changes (restore with 'hg unshelve --abort')
  rebasing shelved changes
  merging f
  warning: conflicts during merge.
  merging f incomplete! (edit conflicts, then use 'hg resolve --mark')
  unresolved conflicts (see 'hg resolve', then 'hg unshelve --continue')
  [1]
  $ hg log -G --template '{rev}  {desc|firstline}  {author}  {date|isodate}'
  @  5  changes to 'commit stuff'  shelve@localhost  1970-01-01 00:00 +0000
  |
  | @  4  pending changes temporary commit  shelve@localhost  2004-01-10 13:37 +0000
  |/
  o  3  commit stuff  test  1970-01-01 00:00 +0000
  |
  | o  2  c  test  1970-01-01 00:00 +0000
  |/
  o  0  a  test  1970-01-01 00:00 +0000
  
  $ hg st
  M f
  ? f.orig
  $ cat f
  <<<<<<< dest:   5f6b880e719b  - shelve: pending changes temporary commit
  g
  =======
  f
  >>>>>>> source: 23b29cada8ba - shelve: changes to 'commit stuff'
  $ cat f.orig
  g
  $ hg unshelve --abort
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
  $ hg st
  M a
  A f
  ? f.orig

 other committed changes - merge:

  $ hg shelve f
  shelved as default
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg ci a -m 'intermediate other change'
  $ mv f.orig f
  $ hg unshelve
  unshelving change 'default'
  rebasing shelved changes
  merging f
  warning: conflicts during merge.
  merging f incomplete! (edit conflicts, then use 'hg resolve --mark')
  unresolved conflicts (see 'hg resolve', then 'hg unshelve --continue')
  [1]
  $ hg st
  M f
  ? f.orig
  $ cat f
  <<<<<<< dest:   *  - test: intermediate other change (glob)
  g
  =======
  f
  >>>>>>> source: 23b29cada8ba - shelve: changes to 'commit stuff'
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

Recreate some conflict again

  $ cd ../repo
  $ hg up -C -r 3
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark test)
  $ echo y >> a/a
  $ hg shelve
  shelved as default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg up test
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark test)
  $ hg unshelve
  unshelving change 'default'
  rebasing shelved changes
  merging a/a
  warning: conflicts during merge.
  merging a/a incomplete! (edit conflicts, then use 'hg resolve --mark')
  unresolved conflicts (see 'hg resolve', then 'hg unshelve --continue')
  [1]

Test that resolving all conflicts in one direction (so that the rebase
is a no-op), works (issue4398)

  $ hg revert -a -r .
  reverting a/a (glob)
  $ hg resolve -m a/a
  (no more unresolved files)
  $ hg unshelve -c
  unshelve of 'default' complete
  $ hg diff
  $ hg status
  ? a/a.orig
  ? foo/foo
  $ hg summary
  parent: 4:33f7f61e6c5e tip
   create conflict
  branch: default
  bookmarks: *test
  commit: 2 unknown (clean)
  update: (current)

  $ hg shelve --delete --stat
  abort: options '--delete' and '--stat' may not be used together
  [255]
  $ hg shelve --delete --name NAME
  abort: options '--delete' and '--name' may not be used together
  [255]

  $ cd ..
