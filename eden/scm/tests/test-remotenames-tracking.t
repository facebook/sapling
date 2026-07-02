#inprocess-hg-incompatible
#require no-eden
  $ setconfig devel.segmented-changelog-rev-compat=true
  $ setconfig ui.ignorerevnum=true

Set up extension and repos

  $ eagerepo
  $ setconfig phases.publish=false
  $ sl init repo1

Make sure we don't fail when rebase doesn't exist

  $ sl rebase
  unknown command 'rebase'
  (use 'sl help' to get help)
  [255]
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > rebase=
  > EOF

Create a tracking bookmark

  $ cd repo1
  $ echo a > a
  $ sl add a
  $ sl commit -m a1
  $ echo a >> a
  $ sl commit -m a2
  $ sl book a
  $ sl up ".^"
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark a)
  $ echo b > b
  $ echo b > a
  $ sl add b
  $ sl commit -m b
  $ sl book b -t a
  $ sl log -G -T '{desc} {bookmarks} {remotebookmarks}\n'
  @  b b
  │
  │ o  a2 a
  ├─╯
  o  a1
  
  $ sl book -v
     a                         fdceb0e57656
   * b                         dea4e1d2ca0e            [a: 1 ahead, 1 behind]
  $ sl rebase -d a --tool :fail
  rebasing dea4e1d2ca0e "b" (b)
  unresolved conflicts (see sl resolve, then sl rebase --continue)
  [1]
  $ echo aa > a
  $ sl resolve --mark a 2>&1 | grep -v "^continue:"
  (no more unresolved files)
  $ sl rebase --continue
  rebasing dea4e1d2ca0e "b" (b)
  $ sl log -G -T '{desc} {bookmarks} {remotebookmarks}'
  @  b b
  │
  o  a2 a
  │
  o  a1
  
  $ sl book -v
     a                         fdceb0e57656
   * b                         2623fce7de21            [a: 1 ahead, 0 behind]

Test push tracking

  $ cd ..
  $ newclientrepo repo2 repo1 a b
  $ setconfig 'remotenames.selectivepulldefault=a b'
  $ sl log -G -T '{desc} {bookmarks} {remotebookmarks}\n'
  @  b  remote/b
  │
  o  a2  remote/a
  │
  o  a1
  

  $ sl bookmarks c -t remote/b
  $ echo c > c
  $ sl add c
  $ sl commit -m c
  $ sl log -G -T '{desc} {bookmarks} {remotebookmarks}\n'
  @  c c
  │
  o  b  remote/b
  │
  o  a2  remote/a
  │
  o  a1
  
  $ sl push
  pushing rev e305ab9fea99 to destination test:repo1 bookmark b
  searching for changes
  updating bookmark b
  $ sl log -G -T '{desc} {bookmarks} {remotebookmarks}\n'
  @  c c remote/b
  │
  o  b
  │
  o  a2  remote/a
  │
  o  a1
  
Test push with explicit default path

  $ sl push `sl paths default`
  pushing rev e305ab9fea99 to destination test:repo1 bookmark b
  searching for changes
  remote bookmark already points at pushed rev

Test that we don't try to push if tracking bookmark isn't a remote bookmark

  $ setconfig remotenames.forceto=true
  $ sl book c -t foo
  $ sl push
  abort: must specify --to when pushing
  (see configuration option remotenames.forceto)
  [255]

Test renaming a remote and tracking

  $ sl dbsh -c "with repo.lock(), repo.transaction('tr'): repo.svfs.writeutf8('remotenames', '')"
  $ setconfig remotenames.rename.default=remote
  $ sl pull
  pulling from test:repo1
  $ sl book c -t remote/a
  $ sl log -G -T '{desc} {bookmarks} {remotebookmarks}\n'
  @  c c remote/b
  │
  o  b
  │
  o  a2  remote/a
  │
  o  a1
  
  $ sl push
  pushing rev e305ab9fea99 to destination test:repo1 bookmark a
  searching for changes
  updating bookmark a
  $ sl log -G -T '{desc} {bookmarks} {remotebookmarks}\n'
  @  c c remote/a remote/b
  │
  o  b
  │
  o  a2
  │
  o  a1
  

Test untracking

  $ sl book -v
   * c                         e305ab9fea99            [remote/a]
  $ sl book -u c
  $ sl book -v
   * c                         e305ab9fea99

Test that tracking isn't over-eager on rebase

  $ sl up 'desc(a2)'
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved
  (leaving bookmark c)
  $ touch e
  $ sl commit -qAm e
  $ sl book c -r 'desc(a2)' -t remote/a -f
  $ sl up c
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (activating bookmark c)
  $ touch d
  $ sl commit -qAm d
  $ sl log -G -T '{node|short} {bookmarks} {remotebookmarks}\n'
  @  ff58066d17c3 c
  │
  │ o  364e447d28f4
  ├─╯
  │ o  e305ab9fea99  remote/a remote/b
  │ │
  │ o  2623fce7de21
  ├─╯
  o  fdceb0e57656
  │
  o  07199ae38cd5
  
  $ sl bookmarks -v
   * c                         ff58066d17c3            [remote/a: 1 ahead, 2 behind]
  $ sl rebase -s .
  abort: you must specify a destination (-d) for the rebase
  [255]
  $ sl log -G -T '{node|short} {bookmarks} {remotebookmarks}\n'
  @  ff58066d17c3 c
  │
  │ o  364e447d28f4
  ├─╯
  │ o  e305ab9fea99  remote/a remote/b
  │ │
  │ o  2623fce7de21
  ├─╯
  o  fdceb0e57656
  │
  o  07199ae38cd5
  
Test implicit rebase destination

  $ sl log -G -T '{node|short} {bookmarks} {remotebookmarks}\n'
  @  ff58066d17c3 c
  │
  │ o  364e447d28f4
  ├─╯
  │ o  e305ab9fea99  remote/a remote/b
  │ │
  │ o  2623fce7de21
  ├─╯
  o  fdceb0e57656
  │
  o  07199ae38cd5
  
  $ sl bookmarks -v
   * c                         ff58066d17c3            [remote/a: 1 ahead, 2 behind]
  $ sl rebase
  rebasing ff58066d17c3 "d" (c)
  $ sl log -G -T '{node|short} {bookmarks} {remotebookmarks}\n'
  @  8d13dc14fef1 c
  │
  │ o  364e447d28f4
  │ │
  o │  e305ab9fea99  remote/a remote/b
  │ │
  o │  2623fce7de21
  ├─╯
  o  fdceb0e57656
  │
  o  07199ae38cd5
  

Test distance to tip calculation

  $ test -f .sl/cache/distance.current
  [1]
  $ sl up 'desc(c)'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (leaving bookmark c)
  $ cat .sl/cache/distance.current
  c 1 (no-eol)
  $ sl up 'desc(b)'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ cat .sl/cache/distance.current
  c 2 (no-eol)
  $ sl up 'desc(e)'
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ test -f .sl/cache/distance.current
  [1]
  $ sl up c
  4 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (activating bookmark c)

Test when a local bookmark that was tracking goes missing

  $ sl book -v
   * c                         8d13dc14fef1            [remote/a: 1 ahead, 0 behind]
  $ sl book -d c
  $ sl book d
  $ sl book -v
   * d                         8d13dc14fef1

Test renaming a bookmark with tracking

  $ sl book d -t remote/a
  $ sl book -v
   * d                         8d13dc14fef1            [remote/a: 1 ahead, 0 behind]
  $ sl book -m d x
  $ sl book -v
   * x                         8d13dc14fef1            [remote/a: 1 ahead, 0 behind]

Test renaming a bookmark without tracking

  $ sl book -u x
  $ sl book -v
   * x                         8d13dc14fef1
  $ sl book -m x d
  $ sl book -v
   * d                         8d13dc14fef1
  $ sl book -d d

Test bookmarks with difficult characters

  $ sl book -t remote/a "bookmark with spaces"
  $ sl book -t remote/b "with	tab too"
  $ sl book -t remote/a "bookmark/with/slashes"
  $ sl book -v
     bookmark with spaces      8d13dc14fef1
   * bookmark/with/slashes     8d13dc14fef1            [remote/a: 1 ahead, 0 behind]
     with	tab too              8d13dc14fef1
  $ sl goto bookmark/with/slashes
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ sl book -v
     bookmark with spaces      8d13dc14fef1
   * bookmark/with/slashes     8d13dc14fef1            [remote/a: 1 ahead, 0 behind]
     with	tab too              8d13dc14fef1
