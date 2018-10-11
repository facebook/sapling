  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > reset=
  > EOF

  $ hg init repo
  $ cd repo

  $ echo x > x
  $ hg commit -qAm x
  $ hg book foo

Soft reset should leave pending changes

  $ echo y >> x
  $ hg commit -qAm y
  $ hg log -G -T '{node|short} {bookmarks}\n'
  @  66ee28d0328c foo
  |
  o  b292c1e3311f
  
  $ hg reset ".^"
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/66ee28d0328c-b6ee89e7-backup.hg (glob)
  $ hg log -G -T '{node|short} {bookmarks}\n'
  @  b292c1e3311f foo
  
  $ hg diff
  diff -r b292c1e3311f x
  --- a/x	Thu Jan 01 00:00:00 1970 +0000
  +++ b/x	* (glob)
  @@ -1,1 +1,2 @@
   x
  +y

Clean reset should overwrite all changes

  $ hg commit -qAm y
  $ hg reset --clean ".^"
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/66ee28d0328c-b6ee89e7-backup.hg (glob)
  $ hg diff

Reset should recover from backup bundles (with correct phase)

  $ hg log -G -T '{node|short} {bookmarks}\n'
  @  b292c1e3311f foo
  
  $ hg phase -p b292c1e3311f
  $ hg reset --clean 66ee28d0328c
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets 66ee28d0328c
  $ hg log -G -T '{node|short} {bookmarks} {phase}\n'
  @  66ee28d0328c foo draft
  |
  o  b292c1e3311f  public
  
  $ hg phase -f -d b292c1e3311f

Reset should not strip reachable commits

  $ hg book bar
  $ hg reset --clean ".^"
  $ hg log -G -T '{node|short} {bookmarks}\n'
  o  66ee28d0328c foo
  |
  @  b292c1e3311f bar
  

  $ hg book -d bar
  $ hg up foo
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark foo)

Reset to '.' by default

  $ echo z >> x
  $ echo z >> y
  $ hg add y
  $ hg st
  M x
  A y
  $ hg reset
  $ hg st
  M x
  ? y
  $ hg reset -C
  $ hg st
  ? y
  $ rm y

Keep old commits

  $ hg reset --keep ".^"
  $ hg log -G -T '{node|short} {bookmarks}\n'
  o  66ee28d0328c
  |
  @  b292c1e3311f foo
  
Reset without a bookmark

  $ hg up tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark foo)
  $ hg book -d foo
  $ hg reset ".^"
  resetting without an active bookmark
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/66ee28d0328c-b6ee89e7-backup.hg (glob)
  $ hg book foo

Reset to bookmark with - in the name

  $ hg reset 66ee28d0328c
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets 66ee28d0328c
  $ hg book foo-bar -r ".^"
  $ hg reset foo-bar
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/66ee28d0328c-b6ee89e7-backup.hg (glob)
  $ hg book -d foo-bar

Verify file status after reset

  $ hg reset -C 66ee28d0328c
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets 66ee28d0328c
  $ touch toberemoved
  $ hg commit -qAm 'add file for removal'
  $ echo z >> x
  $ touch tobeadded
  $ hg add tobeadded
  $ hg rm toberemoved
  $ hg commit -m 'to be reset'
  $ hg reset ".^"
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/d36bf00ac47e-375e6009-backup.hg (glob)
  $ hg status
  M x
  ! toberemoved
  ? tobeadded
  $ hg reset -C 66ee28d0328c
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/34fb347b2aae-c2a02721-backup.hg (glob)

Reset + Obsolete tests

  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > amend=
  > rebase=
  > [experimental]
  > evolution=all
  > EOF
  $ touch a
  $ hg commit -Aqm a
  $ hg log -G -T '{node|short} {bookmarks}\n'
  @  7f3a02b3e388 foo
  |
  o  66ee28d0328c
  |
  o  b292c1e3311f
  

Reset prunes commits

  $ hg reset -C "66ee28d0328c^"
  2 changesets pruned
  $ hg log -r 66ee28d0328c
  abort: hidden revision '66ee28d0328c'!
  (use --hidden to access hidden revisions)
  [255]
  $ hg log -G -T '{node|short} {bookmarks}\n'
  @  b292c1e3311f foo
  
  $ hg reset -C 7f3a02b3e388
  $ hg log -G -T '{node|short} {bookmarks}\n'
  @  7f3a02b3e388 foo
  |
  o  66ee28d0328c
  |
  o  b292c1e3311f
  
Reset to the commit your on is a no-op
  $ hg status
  $ hg log -r . -T '{rev}\n'
  2
  $ hg reset .
  $ hg log -r . -T '{rev}\n'
  2
  $ hg debugdirstate
  n 644          0 * a (glob)
  n 644          0 * tobeadded (glob)
  n 644          4 * x (glob)
