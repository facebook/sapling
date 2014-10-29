  $ extpath=$(dirname $TESTDIR)
  $ cp $extpath/reset.py $TESTTMP # use $TESTTMP substitution in message
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > reset=$TESTTMP/reset.py
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
  
  $ hg reset .^
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/66ee28d0328c-backup.hg
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
  $ hg reset --clean .^
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/66ee28d0328c-backup.hg
  $ hg diff

Reset should recover from backup bundles

  $ hg log -G -T '{node|short} {bookmarks}\n'
  @  b292c1e3311f foo
  
  $ hg reset --clean 66ee28d0328c
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  $ hg log -G -T '{node|short} {bookmarks}\n'
  @  66ee28d0328c foo
  |
  o  b292c1e3311f
  

Reset should not strip reachable commits

  $ hg book bar
  $ hg reset --clean .^
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

  $ hg reset --keep .^
  $ hg log -G -T '{node|short} {bookmarks}\n'
  o  66ee28d0328c
  |
  @  b292c1e3311f foo
  
Reset without a bookmark

  $ hg up tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark foo)
  $ hg book -d foo
  $ hg reset .^
  reseting without an active bookmark
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/66ee28d0328c-backup.hg
  $ hg book foo

Verify file status after reset

  $ hg reset -C 66ee28d0328c
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  $ touch toberemoved
  $ hg commit -qAm 'add file for removal'
  $ echo z >> x
  $ touch tobeadded
  $ hg add tobeadded
  $ hg rm toberemoved
  $ hg commit -m 'to be reset'
  $ hg reset .^
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/d36bf00ac47e-backup.hg
  $ hg status
  M x
  ! toberemoved
  ? tobeadded
