  $ setconfig extensions.treemanifest=!
TODO: Make this test compatibile with obsstore enabled.
  $ setconfig experimental.evolution=
Set up extension and repos

  $ echo "[phases]" >> $HGRCPATH
  $ echo "publish = False" >> $HGRCPATH
  $ echo "[extensions]" >> $HGRCPATH
  $ echo "remotenames=" >> $HGRCPATH
  $ hg init repo1

Make sure we don't fail when rebase doesn't exist

  $ hg rebase
  unknown command 'rebase'
  (use 'hg help' to get help)
  [255]
  $ echo "rebase=" >> $HGRCPATH

Create a tracking bookmark

  $ cd repo1
  $ echo a > a
  $ hg add a
  $ hg commit -m a1
  $ echo a >> a
  $ hg commit -m a2
  $ hg book a
  $ hg up ".^"
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark a)
  $ echo b > b
  $ echo b > a
  $ hg add b
  $ hg commit -m b
  $ hg book b -t a
  $ hg log -G -T '{rev} {desc} {bookmarks} {remotebookmarks}\n'
  @  2 b b
  |
  | o  1 a2 a
  |/
  o  0 a1
  
  $ hg book -v
     a                         1:fdceb0e57656
   * b                         2:dea4e1d2ca0e            [a: 1 ahead, 1 behind]
  $ hg rebase --tool :fail
  rebasing dea4e1d2ca0e "b" (b tip)
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ echo aa > a
  $ hg resolve --mark a 2>&1 | grep -v "^continue:"
  (no more unresolved files)
  $ hg rebase --continue
  rebasing dea4e1d2ca0e "b" (b tip)
  saved backup bundle to $TESTTMP/repo1/.hg/strip-backup/dea4e1d2ca0e-a7f1cee1-*.hg (glob)
  $ hg log -G -T '{rev} {desc} {bookmarks} {remotebookmarks}'
  @  2 b b
  |
  o  1 a2 a
  |
  o  0 a1
  
  $ hg book -v
     a                         1:fdceb0e57656
   * b                         2:2623fce7de21            [a: 1 ahead, 0 behind]

Test push tracking

  $ cd ..
  $ hg clone repo1 repo2
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd repo2
  $ hg log -G -T '{rev} {desc} {bookmarks} {remotebookmarks}\n'
  @  2 b  default/b
  |
  o  1 a2  default/a
  |
  o  0 a1
  

  $ hg boo c -t default/b
  $ echo c > c
  $ hg add c
  $ hg commit -m c
  $ hg log -G -T '{rev} {desc} {bookmarks} {remotebookmarks}\n'
  @  3 c c
  |
  o  2 b  default/b
  |
  o  1 a2  default/a
  |
  o  0 a1
  
  $ hg push
  pushing rev e305ab9fea99 to destination $TESTTMP/repo1 bookmark b
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  updating bookmark b
  $ hg log -G -T '{rev} {desc} {bookmarks} {remotebookmarks}\n'
  @  3 c c default/b
  |
  o  2 b
  |
  o  1 a2  default/a
  |
  o  0 a1
  
Test push with explicit default path

  $ hg push `hg paths default`
  pushing rev e305ab9fea99 to destination $TESTTMP/repo1 bookmark b
  searching for changes
  remote bookmark already points at pushed rev
  no changes found
  [1]

Test that we don't try to push if tracking bookmark isn't a remote bookmark

  $ echo "[remotenames]" >> $HGRCPATH
  $ echo "forceto = True" >> $HGRCPATH
  $ hg book c -t foo
  $ hg push
  abort: must specify --to when pushing
  (see configuration option remotenames.forceto)
  [255]

Test renaming a remote and tracking

  $ rm .hg/store/remotenames
  $ echo "[remotenames]" >> $HGRCPATH
  $ echo "rename.default = remote" >> $HGRCPATH
  $ hg pull
  pulling from $TESTTMP/repo1 (glob)
  searching for changes
  no changes found
  $ hg book c -t remote/a
  $ hg log -G -T '{rev} {desc} {bookmarks} {remotebookmarks}\n'
  @  3 c c remote/b
  |
  o  2 b
  |
  o  1 a2  remote/a
  |
  o  0 a1
  
  $ hg push
  pushing rev e305ab9fea99 to destination $TESTTMP/repo1 bookmark a
  searching for changes
  no changes found
  updating bookmark a
  [1]
  $ hg log -G -T '{rev} {desc} {bookmarks} {remotebookmarks}\n'
  @  3 c c remote/a remote/b
  |
  o  2 b
  |
  o  1 a2
  |
  o  0 a1
  

Test untracking

  $ hg book -v
   * c                         3:e305ab9fea99            [remote/a]
  $ hg book -u c
  $ hg book -v
   * c                         3:e305ab9fea99

Test that tracking isn't over-eager on rebase

  $ hg up 1
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved
  (leaving bookmark c)
  $ touch e
  $ hg commit -qAm e
  $ hg book c -r 1 -t remote/a -f
  $ hg up c
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (activating bookmark c)
  $ touch d
  $ hg commit -qAm d
  $ hg log -G -T '{rev} {node|short} {bookmarks} {remotebookmarks}\n'
  @  5 ff58066d17c3 c
  |
  | o  4 364e447d28f4
  |/
  | o  3 e305ab9fea99  remote/a remote/b
  | |
  | o  2 2623fce7de21
  |/
  o  1 fdceb0e57656
  |
  o  0 07199ae38cd5
  
  $ hg bookmarks -v
   * c                         5:ff58066d17c3            [remote/a: 1 ahead, 2 behind]
  $ hg rebase -s .
  abort: no matching bookmark to rebase - please rebase to an explicit rev or bookmark
  (run 'hg heads' to see all heads)
  [255]
  $ hg log -G -T '{rev} {node|short} {bookmarks} {remotebookmarks}\n'
  @  5 ff58066d17c3 c
  |
  | o  4 364e447d28f4
  |/
  | o  3 e305ab9fea99  remote/a remote/b
  | |
  | o  2 2623fce7de21
  |/
  o  1 fdceb0e57656
  |
  o  0 07199ae38cd5
  
Test implicit rebase destination

  $ hg log -G -T '{rev} {node|short} {bookmarks} {remotebookmarks}\n'
  @  5 ff58066d17c3 c
  |
  | o  4 364e447d28f4
  |/
  | o  3 e305ab9fea99  remote/a remote/b
  | |
  | o  2 2623fce7de21
  |/
  o  1 fdceb0e57656
  |
  o  0 07199ae38cd5
  
  $ hg bookmarks -v
   * c                         5:ff58066d17c3            [remote/a: 1 ahead, 2 behind]
  $ hg rebase
  rebasing ff58066d17c3 "d" (c tip)
  saved backup bundle to $TESTTMP/repo2/.hg/strip-backup/ff58066d17c3-470dd0be-*.hg (glob)
  $ hg log -G -T '{rev} {node|short} {bookmarks} {remotebookmarks}\n'
  @  5 8d13dc14fef1 c
  |
  | o  4 364e447d28f4
  | |
  o |  3 e305ab9fea99  remote/a remote/b
  | |
  o |  2 2623fce7de21
  |/
  o  1 fdceb0e57656
  |
  o  0 07199ae38cd5
  

Test distance to tip calculation

  $ test -f .hg/cache/distance.current
  [1]
  $ hg up 3
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (leaving bookmark c)
  $ cat .hg/cache/distance.current
  c 1 (no-eol)
  $ hg up 2
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ cat .hg/cache/distance.current
  c 2 (no-eol)
  $ hg up 4
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ test -f .hg/cache/distance.current
  [1]
  $ hg up c
  4 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (activating bookmark c)

Test when a local bookmark that was tracking goes missing

  $ hg book -v
   * c                         5:8d13dc14fef1            [remote/a: 1 ahead, 0 behind]
  $ rm .hg/bookmarks
  $ hg book d
  $ hg book -v
   * d                         5:8d13dc14fef1

Test renaming a bookmark with tracking

  $ hg book d -t remote/a
  $ hg book -v
   * d                         5:8d13dc14fef1            [remote/a: 1 ahead, 0 behind]
  $ hg book -m d x
  $ hg book -v
   * x                         5:8d13dc14fef1            [remote/a: 1 ahead, 0 behind]

Test renaming a bookmark without tracking

  $ hg book -u x
  $ hg book -v
   * x                         5:8d13dc14fef1
  $ hg book -m x d
  $ hg book -v
   * d                         5:8d13dc14fef1
  $ hg book -d d

Test bookmarks with difficult characters

  $ hg book -t remote/a "bookmark with spaces"
  $ hg book -t remote/b "with	tab too"
  $ hg book -t remote/a "bookmark/with/slashes"
  $ hg book -v
     bookmark with spaces      5:8d13dc14fef1
   * bookmark/with/slashes     5:8d13dc14fef1            [remote/a: 1 ahead, 0 behind]
     with	tab too              5:8d13dc14fef1
  $ hg update bookmark/with/slashes
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg book -v
     bookmark with spaces      5:8d13dc14fef1
   * bookmark/with/slashes     5:8d13dc14fef1            [remote/a: 1 ahead, 0 behind]
     with	tab too              5:8d13dc14fef1
