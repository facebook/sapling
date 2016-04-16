  $ cat <<EOF >> $HGRCPATH
  > [extensions]
  > mq =
  > shelve =
  > [defaults]
  > diff = --nodates --git
  > qnew = --date '0 0'
  > [shelve]
  > maxbackups = 2
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

shelve has a help message
  $ hg shelve -h
  hg shelve [OPTION]... [FILE]...
  
  save and set aside changes from the working directory
  
      Shelving takes files that "hg status" reports as not clean, saves the
      modifications to a bundle (a shelved change), and reverts the files so
      that their state in the working directory becomes clean.
  
      To restore these changes to the working directory, using "hg unshelve";
      this will work even if you switch to a different commit.
  
      When no files are specified, "hg shelve" saves all not-clean files. If
      specific files or directories are named, only changes to those files are
      shelved.
  
      In bare shelve(when no files are specified, without interactive, include
      and exclude option), shelving remembers information if the working
      directory was on newly created branch, in other words working directory
      was on different branch than its first parent. In this situation
      unshelving restores branch information to the working directory.
  
      Each shelved change has a name that makes it easier to find later. The
      name of a shelved change defaults to being based on the active bookmark,
      or if there is no active bookmark, the current named branch.  To specify a
      different name, use "--name".
  
      To see a list of existing shelved changes, use the "--list" option. For
      each shelved change, this will print its name, age, and description; use "
      --patch" or "--stat" for more details.
  
      To delete specific shelved changes, use "--delete". To delete all shelved
      changes, use "--cleanup".
  
  (use "hg help -e shelve" to show help for the shelve extension)
  
  options ([+] can be repeated):
  
   -A --addremove           mark new/missing files as added/removed before
                            shelving
   -u --unknown             store unknown files in the shelve
      --cleanup             delete all shelved changes
      --date DATE           shelve with the specified commit date
   -d --delete              delete the named shelved change(s)
   -e --edit                invoke editor on commit messages
   -l --list                list current shelves
   -m --message TEXT        use text as shelve message
   -n --name NAME           use the given name for the shelved commit
   -p --patch               show patch
   -i --interactive         interactive mode, only works while creating a shelve
      --stat                output diffstat-style summary of changes
   -I --include PATTERN [+] include names matching the given patterns
   -X --exclude PATTERN [+] exclude names matching the given patterns
      --mq                  operate on patch repository
  
  (some details hidden, use --verbose to show complete help)

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

make sure shelve files were backed up

  $ ls .hg/shelve-backup
  default.hg
  default.patch

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
  default-01      (*)* changes to: [mq]: second.patch (glob)
  default         (*)* changes to: [mq]: second.patch (glob)

  $ hg shelve -l -p default
  default         (*)* changes to: [mq]: second.patch (glob)
  
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

ensure shelve backups aren't overwritten

  $ ls .hg/shelve-backup/
  default-1.hg
  default-1.patch
  default.hg
  default.patch

local edits should not prevent a shelved change from applying

  $ printf "z\na\n" > a/a
  $ hg unshelve --keep
  unshelving change 'default-01'
  temporarily committing pending changes (restore with 'hg unshelve --abort')
  rebasing shelved changes
  rebasing 4:32c69314e062 "changes to: [mq]: second.patch" (tip)
  merging a/a

  $ hg revert --all -q
  $ rm a/a.orig b.rename/b c.copy

apply it and make sure our state is as expected

(this also tests that same timestamp prevents backups from being
removed, even though there are more than 'maxbackups' backups)

  $ f -t .hg/shelve-backup/default.hg
  .hg/shelve-backup/default.hg: file
  $ touch -t 200001010000 .hg/shelve-backup/default.hg
  $ f -t .hg/shelve-backup/default-1.hg
  .hg/shelve-backup/default-1.hg: file
  $ touch -t 200001010000 .hg/shelve-backup/default-1.hg

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

(both of default.hg and default-1.hg should be still kept, because it
is difficult to decide actual order of them from same timestamp)

  $ ls .hg/shelve-backup/
  default-01.hg
  default-01.patch
  default-1.hg
  default-1.patch
  default.hg
  default.patch

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

ensure old shelve backups are being deleted automatically

  $ ls .hg/shelve-backup/
  default-01.hg
  default-01.patch
  wibble.hg
  wibble.patch

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
  rebasing 5:32c69314e062 "changes to: [mq]: second.patch" (tip)
  merging a/a
  warning: conflicts while merging a/a! (edit, then use 'hg resolve --mark')
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
  +>>>>>>> source: 32c69314e062 - shelve: changes to: [mq]: second.patch
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
  abort: no unshelve in progress
  [255]
  $ hg status
  A foo/foo
  ? a/a.orig

redo the unshelve to get a conflict

  $ hg unshelve -q
  warning: conflicts while merging a/a! (edit, then use 'hg resolve --mark')
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
  continue: hg unshelve --continue

  $ hg commit -m 'commit while unshelve in progress'
  abort: unshelve already in progress
  (use 'hg unshelve --continue' or 'hg unshelve --abort')
  [255]

  $ hg graft --continue
  abort: no graft in progress
  (continue: hg unshelve --continue)
  [255]
  $ hg unshelve -c
  rebasing 5:32c69314e062 "changes to: [mq]: second.patch" (tip)
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

  $ hg unshelve --tool :merge-other --keep
  unshelving change 'default'
  temporarily committing pending changes (restore with 'hg unshelve --abort')
  rebasing shelved changes
  rebasing 6:2f694dd83a13 "changes to: second" (tip)
  merging a/a
  $ hg parents -q
  4:33f7f61e6c5e
  $ hg shelve -l
  default         (*)* changes to: second (glob)
  $ hg status
  M a/a
  A foo/foo
  $ cat a/a
  a
  c
  a
  $ cat > a/a << EOF
  > a
  > c
  > x
  > EOF

  $ HGMERGE=true hg unshelve
  unshelving change 'default'
  temporarily committing pending changes (restore with 'hg unshelve --abort')
  rebasing shelved changes
  rebasing 6:2f694dd83a13 "changes to: second" (tip)
  merging a/a
  note: rebase of 6:2f694dd83a13 created no changes to commit
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
  default         (*)* changes to: create conflict (glob)
  $ hg unshelve -k
  unshelving change 'default'
  $ hg shelve --list
  default         (*)* changes to: create conflict (glob)
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
  test            (*)* changes to: create conflict (glob)
  $ hg bookmark
   * test                      4:33f7f61e6c5e
  $ hg --config extensions.mq=! unshelve
  unshelving change 'test'
  $ hg bookmark
   * test                      4:33f7f61e6c5e

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
  rebasing 2:323bfa07f744 "xyz" (tip)
  merging x
  saved backup bundle to $TESTTMP/shelverebase/.hg/strip-backup/323bfa07f744-78114325-backup.hg (glob)
  $ hg unshelve
  unshelving change 'default'
  rebasing shelved changes
  rebasing 4:82a0d7d6ba61 "changes to: xyz" (tip)
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
  rebasing 3:958bcbd1776e "changes to: c" (tip)
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
  rebasing 3:013284d9655e "changes to: b" (tip)
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
  o  4  changes to: commit stuff  shelve@localhost
  |
  ~
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
  rebasing 5:81152db69da7 "changes to: commit stuff" (tip)
  merging f
  warning: conflicts while merging f! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see 'hg resolve', then 'hg unshelve --continue')
  [1]
  $ hg log -G --template '{rev}  {desc|firstline}  {author}  {date|isodate}'
  @  5  changes to: commit stuff  shelve@localhost  1970-01-01 00:00 +0000
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
  >>>>>>> source: 81152db69da7 - shelve: changes to: commit stuff
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
  rebasing 5:81152db69da7 "changes to: commit stuff" (tip)
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
  rebasing 5:81152db69da7 "changes to: commit stuff" (tip)
  merging f
  warning: conflicts while merging f! (edit, then use 'hg resolve --mark')
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
  >>>>>>> source: 81152db69da7 - shelve: changes to: commit stuff
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
  $ hg bookmark
   * test                      4:33f7f61e6c5e
  $ hg unshelve
  unshelving change 'default'
  rebasing shelved changes
  rebasing 5:e42a7da90865 "changes to: second" (tip)
  merging a/a
  warning: conflicts while merging a/a! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see 'hg resolve', then 'hg unshelve --continue')
  [1]
  $ hg bookmark
     test                      4:33f7f61e6c5e

Test that resolving all conflicts in one direction (so that the rebase
is a no-op), works (issue4398)

  $ hg revert -a -r .
  reverting a/a (glob)
  $ hg resolve -m a/a
  (no more unresolved files)
  continue: hg unshelve --continue
  $ hg unshelve -c
  rebasing 5:e42a7da90865 "changes to: second" (tip)
  note: rebase of 5:e42a7da90865 created no changes to commit
  unshelve of 'default' complete
  $ hg bookmark
   * test                      4:33f7f61e6c5e
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
  phases: 5 draft

  $ hg shelve --delete --stat
  abort: options '--delete' and '--stat' may not be used together
  [255]
  $ hg shelve --delete --name NAME
  abort: options '--delete' and '--name' may not be used together
  [255]

Test interactive shelve
  $ cat <<EOF >> $HGRCPATH
  > [ui]
  > interactive = true
  > EOF
  $ echo 'a' >> a/b
  $ cat a/a >> a/b
  $ echo 'x' >> a/b
  $ mv a/b a/a
  $ echo 'a' >> foo/foo
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
  $ hg shelve --interactive --config ui.interactive=false
  abort: running non-interactively
  [255]
  $ hg shelve --interactive << EOF
  > y
  > y
  > n
  > EOF
  diff --git a/a/a b/a/a
  2 hunks, 2 lines changed
  examine changes to 'a/a'? [Ynesfdaq?] y
  
  @@ -1,3 +1,4 @@
  +a
   a
   c
   x
  record change 1/2 to 'a/a'? [Ynesfdaq?] y
  
  @@ -1,3 +2,4 @@
   a
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
   * test                      4:33f7f61e6c5e
  $ hg unshelve
  unshelving change 'test'
  temporarily committing pending changes (restore with 'hg unshelve --abort')
  rebasing shelved changes
  rebasing 6:96a1354f65f6 "changes to: create conflict" (tip)
  merging a/a
  $ hg bookmark
   * test                      4:33f7f61e6c5e
  $ cat a/a
  a
  a
  c
  x
  x

shelve --patch and shelve --stat should work with a single valid shelfname

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
  abort: --patch expects a single shelf
  [255]
  $ hg shelve --stat default default-01
  abort: --stat expects a single shelf
  [255]
  $ hg shelve --patch default
  default         (*)* changes to: create conflict (glob)
  
  diff --git a/shelf-patch-a b/shelf-patch-a
  new file mode 100644
  --- /dev/null
  +++ b/shelf-patch-a
  @@ -0,0 +1,1 @@
  +patch a
  $ hg shelve --stat default
  default         (*)* changes to: create conflict (glob)
   shelf-patch-a |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  $ hg shelve --patch nonexistentshelf
  abort: cannot find shelf nonexistentshelf
  [255]
  $ hg shelve --stat nonexistentshelf
  abort: cannot find shelf nonexistentshelf
  [255]

  $ cd ..

Shelve from general delta repo uses bundle2 on disk
--------------------------------------------------

no general delta

  $ hg clone --pull repo bundle1 --config format.usegeneraldelta=0
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 5 changesets with 8 changes to 6 files
  updating to branch default
  6 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd bundle1
  $ echo babar > jungle
  $ hg add jungle
  $ hg shelve
  shelved as default
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg debugbundle .hg/shelved/*.hg
  45993d65fe9dc3c6d8764b9c3b07fa831ee7d92d
  $ cd ..

with general delta

  $ hg clone --pull repo bundle2 --config format.usegeneraldelta=1
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 5 changesets with 8 changes to 6 files
  updating to branch default
  6 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd bundle2
  $ echo babar > jungle
  $ hg add jungle
  $ hg shelve
  shelved as default
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg debugbundle .hg/shelved/*.hg
  Stream params: {'Compression': 'BZ'}
  changegroup -- "{'version': '02'}"
      45993d65fe9dc3c6d8764b9c3b07fa831ee7d92d
  $ cd ..

Test visibility of in-memory changes inside transaction to external hook
------------------------------------------------------------------------

  $ cd repo

  $ echo xxxx >> x
  $ hg commit -m "#5: changes to invoke rebase"

  $ cat > $TESTTMP/checkvisibility.sh <<EOF
  > echo "==== \$1:"
  > hg parents --template "VISIBLE {rev}:{node|short}\n"
  > # test that pending changes are hidden
  > unset HG_PENDING
  > hg parents --template "ACTUAL  {rev}:{node|short}\n"
  > echo "===="
  > EOF

  $ cat >> .hg/hgrc <<EOF
  > [defaults]
  > # to fix hash id of temporary revisions
  > unshelve = --date '0 0'
  > EOF

"hg unshelve" at REV5 implies steps below:

(1) commit changes in the working directory (REV6)
(2) unbundle shelved revision (REV7)
(3) rebase: merge REV7 into REV6 (REV6 => REV6, REV7)
(4) rebase: commit merged revision (REV8)
(5) rebase: update to REV6 (REV8 => REV6)
(6) update to REV5 (REV6 => REV5)
(7) abort transaction

== test visibility to external preupdate hook

  $ cat >> .hg/hgrc <<EOF
  > [hooks]
  > preupdate.visibility = sh $TESTTMP/checkvisibility.sh preupdate
  > EOF

  $ echo nnnn >> n

  $ sh $TESTTMP/checkvisibility.sh before-unshelving
  ==== before-unshelving:
  VISIBLE 5:703117a2acfb
  ACTUAL  5:703117a2acfb
  ====

  $ hg unshelve --keep default
  temporarily committing pending changes (restore with 'hg unshelve --abort')
  rebasing shelved changes
  rebasing 7:206bf5d4f922 "changes to: create conflict" (tip)
  ==== preupdate:
  VISIBLE 6:66b86db80ee4
  ACTUAL  5:703117a2acfb
  ====
  ==== preupdate:
  VISIBLE 8:a0e04704317e
  ACTUAL  5:703117a2acfb
  ====
  ==== preupdate:
  VISIBLE 6:66b86db80ee4
  ACTUAL  5:703117a2acfb
  ====

  $ cat >> .hg/hgrc <<EOF
  > [hooks]
  > preupdate.visibility =
  > EOF

  $ sh $TESTTMP/checkvisibility.sh after-unshelving
  ==== after-unshelving:
  VISIBLE 5:703117a2acfb
  ACTUAL  5:703117a2acfb
  ====

== test visibility to external update hook

  $ hg update -q -C 5

  $ cat >> .hg/hgrc <<EOF
  > [hooks]
  > update.visibility = sh $TESTTMP/checkvisibility.sh update
  > EOF

  $ echo nnnn >> n

  $ sh $TESTTMP/checkvisibility.sh before-unshelving
  ==== before-unshelving:
  VISIBLE 5:703117a2acfb
  ACTUAL  5:703117a2acfb
  ====

  $ hg unshelve --keep default
  temporarily committing pending changes (restore with 'hg unshelve --abort')
  rebasing shelved changes
  rebasing 7:206bf5d4f922 "changes to: create conflict" (tip)
  ==== update:
  VISIBLE 6:66b86db80ee4
  VISIBLE 7:206bf5d4f922
  ACTUAL  5:703117a2acfb
  ====
  ==== update:
  VISIBLE 6:66b86db80ee4
  ACTUAL  5:703117a2acfb
  ====
  ==== update:
  VISIBLE 5:703117a2acfb
  ACTUAL  5:703117a2acfb
  ====

  $ cat >> .hg/hgrc <<EOF
  > [hooks]
  > update.visibility =
  > EOF

  $ sh $TESTTMP/checkvisibility.sh after-unshelving
  ==== after-unshelving:
  VISIBLE 5:703117a2acfb
  ACTUAL  5:703117a2acfb
  ====

  $ cd ..

test .orig files go where the user wants them to
---------------------------------------------------------------
  $ hg init salvage
  $ cd salvage
  $ echo 'content' > root
  $ hg commit -A -m 'root' -q
  $ echo '' > root
  $ hg shelve -q
  $ echo 'contADDent' > root
  $ hg unshelve -q --config 'ui.origbackuppath=.hg/origbackups'
  warning: conflicts while merging root! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see 'hg resolve', then 'hg unshelve --continue')
  [1]
  $ ls .hg/origbackups
  root.orig
  $ rm -rf .hg/origbackups

test Abort unshelve always gets user out of the unshelved state
---------------------------------------------------------------
Wreak havoc on the unshelve process
  $ rm .hg/unshelverebasestate
  $ hg unshelve --abort
  unshelve of 'default' aborted
  abort: (No such file or directory|The system cannot find the file specified) (re)
  [255]
Can the user leave the current state?
  $ hg up -C .
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

Try again but with a corrupted shelve state file
  $ hg strip -r 2 -r 1 -q
  $ hg up -r 0 -q
  $ echo '' > root
  $ hg shelve -q
  $ echo 'contADDent' > root
  $ hg unshelve -q
  warning: conflicts while merging root! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see 'hg resolve', then 'hg unshelve --continue')
  [1]
  $ sed 's/ae8c668541e8/123456789012/' .hg/shelvedstate > ../corrupt-shelvedstate
  $ mv ../corrupt-shelvedstate .hg/histedit-state
  $ hg unshelve --abort 2>&1 | grep 'rebase aborted'
  rebase aborted
  $ hg up -C .
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cd ..

Keep active bookmark while (un)shelving even on shared repo (issue4940)
-----------------------------------------------------------------------

  $ cat <<EOF >> $HGRCPATH
  > [extensions]
  > share =
  > EOF

  $ hg bookmarks -R repo
     test                      4:33f7f61e6c5e
  $ hg share -B repo share
  updating working directory
  6 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd share

  $ hg bookmarks
     test                      4:33f7f61e6c5e
  $ hg bookmarks foo
  $ hg bookmarks
   * foo                       5:703117a2acfb
     test                      4:33f7f61e6c5e
  $ echo x >> x
  $ hg shelve
  shelved as foo
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bookmarks
   * foo                       5:703117a2acfb
     test                      4:33f7f61e6c5e

  $ hg unshelve
  unshelving change 'foo'
  $ hg bookmarks
   * foo                       5:703117a2acfb
     test                      4:33f7f61e6c5e

  $ cd ..

Shelve and unshelve unknown files. For the purposes of unshelve, a shelved
unknown file is the same as a shelved added file, except that it will be in
unknown state after unshelve if and only if it was either absent or unknown
before the unshelve operation.

  $ hg init unknowns
  $ cd unknowns

The simplest case is if I simply have an unknown file that I shelve and unshelve

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

If I shelve, add the file, and unshelve, does it stay added?

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
  rebasing 1:098df96e7410 "(changes in empty repository)" (tip)
  merging unknown
  $ hg status
  A unknown
  $ hg forget unknown
  $ rm unknown

And if I shelve, commit, then unshelve, does it become modified?

  $ echo unknown > unknown
  $ hg shelve -u
  shelved as default
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg status
  $ touch unknown
  $ hg add unknown
  $ hg commit -qm "Add unknown"
  $ hg status
  $ hg unshelve
  unshelving change 'default'
  rebasing shelved changes
  rebasing 1:098df96e7410 "(changes in empty repository)" (tip)
  merging unknown
  $ hg status
  M unknown
  $ hg remove --force unknown
  $ hg commit -qm "Remove unknown"

  $ cd ..

We expects that non-bare shelve keeps newly created branch in
working directory.

  $ hg init shelve-preserve-new-branch
  $ cd shelve-preserve-new-branch
  $ echo "a" >> a
  $ hg add a
  $ echo "b" >> b
  $ hg add b
  $ hg commit -m "ab"
  $ echo "aa" >> a
  $ echo "bb" >> b
  $ hg branch new-branch
  marked working directory as branch new-branch
  (branches are permanent and global, did you want a bookmark?)
  $ hg status
  M a
  M b
  $ hg branch
  new-branch
  $ hg shelve a
  shelved as default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg branch
  new-branch
  $ hg status
  M b
  $ touch "c" >> c
  $ hg add c
  $ hg status
  M b
  A c
  $ hg shelve --exclude c
  shelved as default-01
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg branch
  new-branch
  $ hg status
  A c
  $ hg shelve --include c
  shelved as default-02
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg branch
  new-branch
  $ hg status
  $ echo "d" >> d
  $ hg add d
  $ hg status
  A d

We expect that bare-shelve will not keep branch in current working directory.

  $ hg shelve
  shelved as default-03
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg branch
  default

When i shelve commit on newly created branch i expect
that after unshelve newly created branch will be preserved.

  $ hg init shelve_on_new_branch_simple
  $ cd shelve_on_new_branch_simple
  $ echo "aaa" >> a
  $ hg commit -A -m "a"
  adding a
  $ hg branch
  default
  $ hg branch test
  marked working directory as branch test
  (branches are permanent and global, did you want a bookmark?)
  $ echo "bbb" >> a
  $ hg status
  M a
  $ hg shelve
  shelved as default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg branch
  default
  $ echo "bbb" >> b
  $ hg status
  ? b
  $ hg unshelve
  unshelving change 'default'
  marked working directory as branch test
  $ hg status
  M a
  ? b
  $ hg branch
  test

When i shelve commit on newly created branch, make
some changes, unshelve it and running into merge
conflicts i expect that after fixing them and
running unshelve --continue newly created branch
will be preserved.

  $ hg init shelve_on_new_branch_conflict
  $ cd shelve_on_new_branch_conflict
  $ echo "aaa" >> a
  $ hg commit -A -m "a"
  adding a
  $ hg branch
  default
  $ hg branch test
  marked working directory as branch test
  (branches are permanent and global, did you want a bookmark?)
  $ echo "bbb" >> a
  $ hg status
  M a
  $ hg shelve
  shelved as default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg branch
  default
  $ echo "ccc" >> a
  $ hg status
  M a
  $ hg unshelve
  unshelving change 'default'
  temporarily committing pending changes (restore with 'hg unshelve --abort')
  rebasing shelved changes
  rebasing 2:425c97ef07f3 "changes to: a" (tip)
  merging a
  warning: conflicts while merging a! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see 'hg resolve', then 'hg unshelve --continue')
  [1]
  $ echo "aaabbbccc" > a
  $ rm a.orig
  $ hg resolve --mark a
  (no more unresolved files)
  continue: hg unshelve --continue
  $ hg unshelve --continue
  rebasing 2:425c97ef07f3 "changes to: a" (tip)
  marked working directory as branch test
  unshelve of 'default' complete
  $ cat a
  aaabbbccc
  $ hg status
  M a
  $ hg branch
  test
  $ hg commit -m "test-commit"

When i shelve on test branch, update to default branch
and unshelve i expect that it will not preserve previous
test branch.

  $ echo "xxx" > b
  $ hg add b
  $ hg shelve
  shelved as test
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg update -r default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg unshelve
  unshelving change 'test'
  rebasing shelved changes
  rebasing 2:357525f34729 "changes to: test-commit" (tip)
  $ hg status
  A b
  $ hg branch
  default

When i unshelve resulting in merge conflicts and makes saved
file shelvedstate looks like in previous versions in
mercurial(without restore branch information in 7th line) i
expect that after resolving conflicts and succesfully
running 'shelve --continue' the branch information won't be
restored and branch will be unchanged.

shelve on new branch, conflict with previous shelvedstate

  $ hg init conflict
  $ cd conflict
  $ echo "aaa" >> a
  $ hg commit -A -m "a"
  adding a
  $ hg branch
  default
  $ hg branch test
  marked working directory as branch test
  (branches are permanent and global, did you want a bookmark?)
  $ echo "bbb" >> a
  $ hg status
  M a
  $ hg shelve
  shelved as default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg branch
  default
  $ echo "ccc" >> a
  $ hg status
  M a
  $ hg unshelve
  unshelving change 'default'
  temporarily committing pending changes (restore with 'hg unshelve --abort')
  rebasing shelved changes
  rebasing 2:425c97ef07f3 "changes to: a" (tip)
  merging a
  warning: conflicts while merging a! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see 'hg resolve', then 'hg unshelve --continue')
  [1]

Removing restore branch information from shelvedstate file(making it looks like
in previous versions) and running unshelve --continue

  $ head -n 6 < .hg/shelvedstate > .hg/shelvedstate_oldformat
  $ rm .hg/shelvedstate
  $ mv .hg/shelvedstate_oldformat .hg/shelvedstate

  $ echo "aaabbbccc" > a
  $ rm a.orig
  $ hg resolve --mark a
  (no more unresolved files)
  continue: hg unshelve --continue
  $ hg unshelve --continue
  rebasing 2:425c97ef07f3 "changes to: a" (tip)
  unshelve of 'default' complete
  $ cat a
  aaabbbccc
  $ hg status
  M a
  $ hg branch
  default

On non bare shelve the branch information shouldn't be restored

  $ hg init bare_shelve_on_new_branch
  $ cd bare_shelve_on_new_branch
  $ echo "aaa" >> a
  $ hg commit -A -m "a"
  adding a
  $ hg branch
  default
  $ hg branch test
  marked working directory as branch test
  (branches are permanent and global, did you want a bookmark?)
  $ echo "bbb" >> a
  $ hg status
  M a
  $ hg shelve a
  shelved as default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg branch
  test
  $ hg branch default
  marked working directory as branch default
  (branches are permanent and global, did you want a bookmark?)
  $ echo "bbb" >> b
  $ hg status
  ? b
  $ hg unshelve
  unshelving change 'default'
  $ hg status
  M a
  ? b
  $ hg branch
  default
