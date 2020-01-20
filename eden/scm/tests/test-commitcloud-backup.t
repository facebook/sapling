#chg-compatible

  $ enable amend
  $ setconfig infinitepushbackup.hostname=testhost
  $ disable treemanifest

  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/infinitepush/library.sh"
  $ setupcommon

Setup server
  $ hg init repo
  $ cd repo
  $ setupserver
  $ cd ..

Backup empty repo
  $ hg clone ssh://user@dummy/repo client -q
  $ cd client
  $ hg cloud backup
  nothing to back up
  $ mkcommit commit
  $ hg prune .
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  working directory now at 000000000000
  1 changesets pruned
  hint[strip-hide]: 'hg strip' may be deprecated in the future - use 'hg hide' instead
  hint[hint-ack]: use 'hg hint --ack strip-hide' to silence these hints
  $ mkcommit newcommit
  $ hg cloud backup
  backing up stack rooted at 606a357e69ad
  remote: pushing 1 commit:
  remote:     606a357e69ad  newcommit
  commitcloud: backed up 1 commit

Re-clone the client
  $ cd ..
  $ rm -rf client
  $ hg clone ssh://user@dummy/repo client -q
  $ cd client

Pushing in this new, empty clone shouldn't clear the old backup
  $ hg cloud backup
  nothing to back up
  $ scratchbookmarks
  infinitepush/backups/test/testhost$TESTTMP/client/heads/606a357e69adb2e36d559ae3237626e82a955c9d 606a357e69adb2e36d559ae3237626e82a955c9d

Make commit and backup it.
  $ mkcommit commit
  $ hg cloud backup
  backing up stack rooted at 7e6a6fd9c7c8
  remote: pushing 1 commit:
  remote:     7e6a6fd9c7c8  commit
  commitcloud: backed up 1 commit
  $ scratchnodes
  606a357e69adb2e36d559ae3237626e82a955c9d 9fa7f02468b18919035248ab21c8267674c0a3d6
  7e6a6fd9c7c8c8c307ee14678f03d63af3a7b455 168423c30397d95ef5f44d883f0887f0f5be0936
  $ scratchbookmarks
  infinitepush/backups/test/testhost$TESTTMP/client/heads/7e6a6fd9c7c8c8c307ee14678f03d63af3a7b455 7e6a6fd9c7c8c8c307ee14678f03d63af3a7b455

Make first commit public (by doing push) and then backup new commit
  $ hg push
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files
  $ mkcommit newcommit
  $ hg cloud backup
  backing up stack rooted at 94a60f5ad8b2
  remote: pushing 1 commit:
  remote:     94a60f5ad8b2  newcommit
  commitcloud: backed up 1 commit
  $ scratchbookmarks
  infinitepush/backups/test/testhost$TESTTMP/client/heads/94a60f5ad8b2e007240007edab982b3638a3f38d 94a60f5ad8b2e007240007edab982b3638a3f38d

Create obsoleted commit
  $ mkcommit obsoletedcommit
  $ hg prune .
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  working directory now at 94a60f5ad8b2
  1 changesets pruned
  hint[strip-hide]: 'hg strip' may be deprecated in the future - use 'hg hide' instead
  hint[hint-ack]: use 'hg hint --ack strip-hide' to silence these hints

Make obsoleted commit non-extinct by committing on top of it
  $ hg --hidden up 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ mkcommit ontopofobsoleted

Backup both of them
  $ hg cloud backup
  backing up stack rooted at 94a60f5ad8b2
  remote: pushing 3 commits:
  remote:     94a60f5ad8b2  newcommit
  remote:     361e89f06232  obsoletedcommit
  remote:     d5609f7fa633  ontopofobsoleted
  commitcloud: backed up 2 commits
  $ scratchbookmarks
  infinitepush/backups/test/testhost$TESTTMP/client/heads/d5609f7fa63352da538eeffbe3ffabed1779aafc d5609f7fa63352da538eeffbe3ffabed1779aafc

Create one more head and run `hg cloud backup`. Make sure that only new head is pushed
  $ hg up 0
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved
  $ mkcommit newhead
  $ hg cloud backup
  backing up stack rooted at 3a30e220fe42
  remote: pushing 1 commit:
  remote:     3a30e220fe42  newhead
  commitcloud: backed up 1 commit

Create two more heads and backup them
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit newhead1
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit newhead2
  $ hg cloud backup
  backing up stack rooted at f79c5017def3
  remote: pushing 1 commit:
  remote:     f79c5017def3  newhead1
  backing up stack rooted at 667453c0787e
  remote: pushing 1 commit:
  remote:     667453c0787e  newhead2
  commitcloud: backed up 2 commits

Backup in background
  $ scratchbookmarks
  infinitepush/backups/test/testhost$TESTTMP/client/heads/3a30e220fe42e969e34bbe8001b951a20f31f2e8 3a30e220fe42e969e34bbe8001b951a20f31f2e8
  infinitepush/backups/test/testhost$TESTTMP/client/heads/667453c0787e7830fdfb86db0f8c29aa7af2a1ea 667453c0787e7830fdfb86db0f8c29aa7af2a1ea
  infinitepush/backups/test/testhost$TESTTMP/client/heads/d5609f7fa63352da538eeffbe3ffabed1779aafc d5609f7fa63352da538eeffbe3ffabed1779aafc
  infinitepush/backups/test/testhost$TESTTMP/client/heads/f79c5017def3b9af9928edbb52cc620c74b4b291 f79c5017def3b9af9928edbb52cc620c74b4b291
  $ mkcommitautobackup newcommit
  $ waitbgbackup
  $ scratchbookmarks
  infinitepush/backups/test/testhost$TESTTMP/client/heads/3a30e220fe42e969e34bbe8001b951a20f31f2e8 3a30e220fe42e969e34bbe8001b951a20f31f2e8
  infinitepush/backups/test/testhost$TESTTMP/client/heads/773a3ba2e7c25358df2e5b3cced70371333bc61c 773a3ba2e7c25358df2e5b3cced70371333bc61c
  infinitepush/backups/test/testhost$TESTTMP/client/heads/d5609f7fa63352da538eeffbe3ffabed1779aafc d5609f7fa63352da538eeffbe3ffabed1779aafc
  infinitepush/backups/test/testhost$TESTTMP/client/heads/f79c5017def3b9af9928edbb52cc620c74b4b291 f79c5017def3b9af9928edbb52cc620c74b4b291

Backup with bookmark
  $ mkcommit commitwithbookmark
  $ hg book abook
  $ hg cloud backup
  backing up stack rooted at 667453c0787e
  remote: pushing 3 commits:
  remote:     667453c0787e  newhead2
  remote:     773a3ba2e7c2  newcommit
  remote:     166ff4468f7d  commitwithbookmark
  commitcloud: backed up 1 commit
  $ scratchbookmarks
  infinitepush/backups/test/testhost$TESTTMP/client/bookmarks/abook 166ff4468f7da443df90d268158ba7d75d52585a
  infinitepush/backups/test/testhost$TESTTMP/client/heads/166ff4468f7da443df90d268158ba7d75d52585a 166ff4468f7da443df90d268158ba7d75d52585a
  infinitepush/backups/test/testhost$TESTTMP/client/heads/3a30e220fe42e969e34bbe8001b951a20f31f2e8 3a30e220fe42e969e34bbe8001b951a20f31f2e8
  infinitepush/backups/test/testhost$TESTTMP/client/heads/d5609f7fa63352da538eeffbe3ffabed1779aafc d5609f7fa63352da538eeffbe3ffabed1779aafc
  infinitepush/backups/test/testhost$TESTTMP/client/heads/f79c5017def3b9af9928edbb52cc620c74b4b291 f79c5017def3b9af9928edbb52cc620c74b4b291

Backup only bookmarks. First, set a low limit to prevent the backup.
  $ hg book newbook
  $ hg cloud backup --config infinitepushbackup.backupbookmarklimit=0
  warning: commitcloud: not pushing backup bookmarks for infinitepush/backups/test/testhost$TESTTMP/client as there are too many (1 > 0)
  nothing to back up
  $ scratchbookmarks | grep newbook
  [1]

New remove the limit and check the backup works.
  $ hg cloud backup
  nothing to back up
  $ scratchbookmarks
  infinitepush/backups/test/testhost$TESTTMP/client/bookmarks/abook 166ff4468f7da443df90d268158ba7d75d52585a
  infinitepush/backups/test/testhost$TESTTMP/client/bookmarks/newbook 166ff4468f7da443df90d268158ba7d75d52585a
  infinitepush/backups/test/testhost$TESTTMP/client/heads/166ff4468f7da443df90d268158ba7d75d52585a 166ff4468f7da443df90d268158ba7d75d52585a
  infinitepush/backups/test/testhost$TESTTMP/client/heads/3a30e220fe42e969e34bbe8001b951a20f31f2e8 3a30e220fe42e969e34bbe8001b951a20f31f2e8
  infinitepush/backups/test/testhost$TESTTMP/client/heads/d5609f7fa63352da538eeffbe3ffabed1779aafc d5609f7fa63352da538eeffbe3ffabed1779aafc
  infinitepush/backups/test/testhost$TESTTMP/client/heads/f79c5017def3b9af9928edbb52cc620c74b4b291 f79c5017def3b9af9928edbb52cc620c74b4b291

Nothing changed, make sure no backup and no connection to the server happens
  $ hg cloud backup --debug
  nothing to back up

Obsolete a head, make sure backup happens
  $ hg prune .
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  working directory now at 773a3ba2e7c2
  1 changesets pruned
  hint[strip-hide]: 'hg strip' may be deprecated in the future - use 'hg hide' instead
  hint[hint-ack]: use 'hg hint --ack strip-hide' to silence these hints
  $ hg cloud backup --traceback
  nothing to back up
  $ scratchbookmarks
  infinitepush/backups/test/testhost$TESTTMP/client/bookmarks/abook 773a3ba2e7c25358df2e5b3cced70371333bc61c
  infinitepush/backups/test/testhost$TESTTMP/client/bookmarks/newbook 773a3ba2e7c25358df2e5b3cced70371333bc61c
  infinitepush/backups/test/testhost$TESTTMP/client/heads/3a30e220fe42e969e34bbe8001b951a20f31f2e8 3a30e220fe42e969e34bbe8001b951a20f31f2e8
  infinitepush/backups/test/testhost$TESTTMP/client/heads/773a3ba2e7c25358df2e5b3cced70371333bc61c 773a3ba2e7c25358df2e5b3cced70371333bc61c
  infinitepush/backups/test/testhost$TESTTMP/client/heads/d5609f7fa63352da538eeffbe3ffabed1779aafc d5609f7fa63352da538eeffbe3ffabed1779aafc
  infinitepush/backups/test/testhost$TESTTMP/client/heads/f79c5017def3b9af9928edbb52cc620c74b4b291 f79c5017def3b9af9928edbb52cc620c74b4b291

Rebase + backup. Make sure that two heads were deleted and head was saved
  $ hg log --graph -T '{node} {desc}'
  @  773a3ba2e7c25358df2e5b3cced70371333bc61c newcommit
  |
  o  667453c0787e7830fdfb86db0f8c29aa7af2a1ea newhead2
  |
  | o  f79c5017def3b9af9928edbb52cc620c74b4b291 newhead1
  |/
  | o  3a30e220fe42e969e34bbe8001b951a20f31f2e8 newhead
  |/
  | o  d5609f7fa63352da538eeffbe3ffabed1779aafc ontopofobsoleted
  | |
  | x  361e89f06232897a098e3a11c49d9d8987da469d obsoletedcommit
  | |
  | o  94a60f5ad8b2e007240007edab982b3638a3f38d newcommit
  |/
  o  7e6a6fd9c7c8c8c307ee14678f03d63af3a7b455 commit
  
  $ hg rebase -s f79c5017de -d 773a3ba2e7c2
  rebasing f79c5017def3 "newhead1"
  $ hg cloud backup
  backing up stack rooted at 667453c0787e
  remote: pushing 3 commits:
  remote:     667453c0787e  newhead2
  remote:     773a3ba2e7c2  newcommit
  remote:     2d2e01441947  newhead1
  commitcloud: backed up 1 commit
  $ scratchbookmarks
  infinitepush/backups/test/testhost$TESTTMP/client/bookmarks/abook 773a3ba2e7c25358df2e5b3cced70371333bc61c
  infinitepush/backups/test/testhost$TESTTMP/client/bookmarks/newbook 773a3ba2e7c25358df2e5b3cced70371333bc61c
  infinitepush/backups/test/testhost$TESTTMP/client/heads/2d2e01441947afbb6bb5ae0efbb901f3eebe3fbd 2d2e01441947afbb6bb5ae0efbb901f3eebe3fbd
  infinitepush/backups/test/testhost$TESTTMP/client/heads/3a30e220fe42e969e34bbe8001b951a20f31f2e8 3a30e220fe42e969e34bbe8001b951a20f31f2e8
  infinitepush/backups/test/testhost$TESTTMP/client/heads/d5609f7fa63352da538eeffbe3ffabed1779aafc d5609f7fa63352da538eeffbe3ffabed1779aafc
Make a few public commits. Make sure we don't backup them
  $ hg up 2d2e01441947
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark newbook)
  $ mkcommit public1
  $ mkcommit public2
  $ hg log -r tip -T '{rev}'
  11 (no-eol)
  $ hg push -r '2d2e01441947::.'
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 5 changesets with 5 changes to 5 files
  $ hg log --graph -T '{node} {desc} {phase}'
  @  3446a384dd701da41cd83cbd9562805fc6412c0e public2 public
  |
  o  bd2412178ef2d3f3aaaf8a4f2385bdd64b5c5e54 public1 public
  |
  o  2d2e01441947afbb6bb5ae0efbb901f3eebe3fbd newhead1 public
  |
  o  773a3ba2e7c25358df2e5b3cced70371333bc61c newcommit public
  |
  o  667453c0787e7830fdfb86db0f8c29aa7af2a1ea newhead2 public
  |
  | o  3a30e220fe42e969e34bbe8001b951a20f31f2e8 newhead draft
  |/
  | o  d5609f7fa63352da538eeffbe3ffabed1779aafc ontopofobsoleted draft
  | |
  | x  361e89f06232897a098e3a11c49d9d8987da469d obsoletedcommit draft
  | |
  | o  94a60f5ad8b2e007240007edab982b3638a3f38d newcommit draft
  |/
  o  7e6a6fd9c7c8c8c307ee14678f03d63af3a7b455 commit public
  
  $ hg cloud backup
  nothing to back up
  $ scratchbookmarks
  infinitepush/backups/test/testhost$TESTTMP/client/bookmarks/abook 773a3ba2e7c25358df2e5b3cced70371333bc61c
  infinitepush/backups/test/testhost$TESTTMP/client/bookmarks/newbook 773a3ba2e7c25358df2e5b3cced70371333bc61c
  infinitepush/backups/test/testhost$TESTTMP/client/heads/3a30e220fe42e969e34bbe8001b951a20f31f2e8 3a30e220fe42e969e34bbe8001b951a20f31f2e8
  infinitepush/backups/test/testhost$TESTTMP/client/heads/d5609f7fa63352da538eeffbe3ffabed1779aafc d5609f7fa63352da538eeffbe3ffabed1779aafc
  $ hg cloud backup
  nothing to back up

Backup bookmark that has '/bookmarks/' in the name. Make sure it was escaped
  $ hg book new/bookmarks/book
  $ hg cloud backup
  nothing to back up
  $ scratchbookmarks
  infinitepush/backups/test/testhost$TESTTMP/client/bookmarks/abook 773a3ba2e7c25358df2e5b3cced70371333bc61c
  infinitepush/backups/test/testhost$TESTTMP/client/bookmarks/new/bookmarksbookmarks/book 3446a384dd701da41cd83cbd9562805fc6412c0e
  infinitepush/backups/test/testhost$TESTTMP/client/bookmarks/newbook 773a3ba2e7c25358df2e5b3cced70371333bc61c
  infinitepush/backups/test/testhost$TESTTMP/client/heads/3a30e220fe42e969e34bbe8001b951a20f31f2e8 3a30e220fe42e969e34bbe8001b951a20f31f2e8
  infinitepush/backups/test/testhost$TESTTMP/client/heads/d5609f7fa63352da538eeffbe3ffabed1779aafc d5609f7fa63352da538eeffbe3ffabed1779aafc

Backup to different path
  $ cat >> .hg/hgrc << EOF
  > [paths]
  > default = brokenpath
  > nondefault = ssh://user@dummy/repo
  > EOF
  $ hg book somebook
  $ hg --config paths.default=brokenpath cloud backup
  abort: repository $TESTTMP/client/brokenpath not found!
  [255]
  $ hg cloud backup --dest nondefault --traceback
  nothing to back up
  $ scratchbookmarks
  infinitepush/backups/test/testhost$TESTTMP/client/bookmarks/abook 773a3ba2e7c25358df2e5b3cced70371333bc61c
  infinitepush/backups/test/testhost$TESTTMP/client/bookmarks/new/bookmarksbookmarks/book 3446a384dd701da41cd83cbd9562805fc6412c0e
  infinitepush/backups/test/testhost$TESTTMP/client/bookmarks/newbook 773a3ba2e7c25358df2e5b3cced70371333bc61c
  infinitepush/backups/test/testhost$TESTTMP/client/bookmarks/somebook 3446a384dd701da41cd83cbd9562805fc6412c0e
  infinitepush/backups/test/testhost$TESTTMP/client/heads/3a30e220fe42e969e34bbe8001b951a20f31f2e8 3a30e220fe42e969e34bbe8001b951a20f31f2e8
  infinitepush/backups/test/testhost$TESTTMP/client/heads/d5609f7fa63352da538eeffbe3ffabed1779aafc d5609f7fa63352da538eeffbe3ffabed1779aafc

Backup in background to different path
  $ mkcommit backgroundcommittodifferentpath
  $ hg cloud backup --dest nondefault --background
  $ waitbgbackup
  $ scratchbookmarks
  infinitepush/backups/test/testhost$TESTTMP/client/bookmarks/abook 773a3ba2e7c25358df2e5b3cced70371333bc61c
  infinitepush/backups/test/testhost$TESTTMP/client/bookmarks/new/bookmarksbookmarks/book 3446a384dd701da41cd83cbd9562805fc6412c0e
  infinitepush/backups/test/testhost$TESTTMP/client/bookmarks/newbook 773a3ba2e7c25358df2e5b3cced70371333bc61c
  infinitepush/backups/test/testhost$TESTTMP/client/bookmarks/somebook 268f86e364f9aed2f5bb9d11e2df6381ace129a2
  infinitepush/backups/test/testhost$TESTTMP/client/heads/268f86e364f9aed2f5bb9d11e2df6381ace129a2 268f86e364f9aed2f5bb9d11e2df6381ace129a2
  infinitepush/backups/test/testhost$TESTTMP/client/heads/3a30e220fe42e969e34bbe8001b951a20f31f2e8 3a30e220fe42e969e34bbe8001b951a20f31f2e8
  infinitepush/backups/test/testhost$TESTTMP/client/heads/d5609f7fa63352da538eeffbe3ffabed1779aafc d5609f7fa63352da538eeffbe3ffabed1779aafc

Clean client and repo
  $ cd ..
  $ rm -rf repo
  $ rm -rf client
  $ hg init repo
  $ cd repo
  $ setupserver
  $ cd ..
  $ hg clone ssh://user@dummy/repo client -q
  $ cd client

Create public commit
  $ mkcommit initial
  $ hg push
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files

Make commit and immediately obsolete it, then create a bookmark.
Make sure cloud backup works
  $ mkcommit toobsolete
  $ hg prune .
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  working directory now at 630839011471
  1 changesets pruned
  hint[strip-hide]: 'hg strip' may be deprecated in the future - use 'hg hide' instead
  hint[hint-ack]: use 'hg hint --ack strip-hide' to silence these hints
  $ hg book somebook
  $ hg cloud backup
  nothing to back up
  $ scratchbookmarks
  infinitepush/backups/test/testhost$TESTTMP/client/bookmarks/somebook 630839011471e17f808b92ab084bedfaca33b110

Make secret commit and bookmark on top of it. Then run cloud backup.
Make sure it was backed up.
t
  $ hg book bookonsecret
  $ echo secret >> secret
  $ hg add secret
  $ hg ci -Am secret
  $ hg phase -qfs '.'
  $ hg cloud backup
  backing up stack rooted at dc80aa94cb8b
  remote: pushing 1 commit:
  remote:     dc80aa94cb8b  secret
  commitcloud: backed up 1 commit
  $ scratchbookmarks
  infinitepush/backups/test/testhost$TESTTMP/client/bookmarks/bookonsecret dc80aa94cb8b16f962a5fb6e56e9ed234644b4e3
  infinitepush/backups/test/testhost$TESTTMP/client/bookmarks/somebook 630839011471e17f808b92ab084bedfaca33b110

Create two heads, set maxheadstobackup to 1, make sure only latest head was backed up
  $ hg up -q 0
  $ mkcommit headone
  $ hg up -q 0
  $ mkcommit headtwo
  $ hg cloud backup --config infinitepushbackup.maxheadstobackup=1
  backing up only the most recent 1 head
  backing up stack rooted at 6c4f4b30ae4c
  remote: pushing 1 commit:
  remote:     6c4f4b30ae4c  headtwo
  commitcloud: backed up 1 commit
  $ scratchbookmarks
  infinitepush/backups/test/testhost$TESTTMP/client/bookmarks/bookonsecret dc80aa94cb8b16f962a5fb6e56e9ed234644b4e3
  infinitepush/backups/test/testhost$TESTTMP/client/bookmarks/somebook 630839011471e17f808b92ab084bedfaca33b110
  infinitepush/backups/test/testhost$TESTTMP/client/heads/6c4f4b30ae4c2dd928d551836c70c741ee836650 6c4f4b30ae4c2dd928d551836c70c741ee836650

Now set maxheadstobackup to 0 and backup again. Make sure nothing is backed up now
  $ hg cloud backup --config infinitepushbackup.maxheadstobackup=0
  backing up only the most recent 0 heads
  nothing to back up
  $ scratchbookmarks
  infinitepush/backups/test/testhost$TESTTMP/client/bookmarks/bookonsecret dc80aa94cb8b16f962a5fb6e56e9ed234644b4e3
  infinitepush/backups/test/testhost$TESTTMP/client/bookmarks/somebook 630839011471e17f808b92ab084bedfaca33b110
  infinitepush/backups/test/testhost$TESTTMP/client/heads/6c4f4b30ae4c2dd928d551836c70c741ee836650 6c4f4b30ae4c2dd928d551836c70c741ee836650

Test cloud check command
  $ hg cloud backup
  backing up stack rooted at cf2adfba1469
  remote: pushing 1 commit:
  remote:     cf2adfba1469  headone
  commitcloud: backed up 1 commit
  $ hg cloud check
  6c4f4b30ae4c2dd928d551836c70c741ee836650 backed up
  $ hg cloud check -r 630839011471e17
  630839011471e17f808b92ab084bedfaca33b110 not backed up
  $ hg cloud check -r . -r 630839011471e17
  6c4f4b30ae4c2dd928d551836c70c741ee836650 backed up
  630839011471e17f808b92ab084bedfaca33b110 not backed up

Delete a commit from the server
  $ rm ../repo/.hg/scratchbranches/index/nodemap/6c4f4b30ae4c2dd928d551836c70c741ee836650

Local state still shows it as backed up, but can check the remote
  $ hg cloud check -r "draft()"
  cf2adfba146909529bcca8c1626de6b4d9e73846 backed up
  6c4f4b30ae4c2dd928d551836c70c741ee836650 backed up
  $ hg cloud check -r "draft()" --remote
  cf2adfba146909529bcca8c1626de6b4d9e73846 backed up
  6c4f4b30ae4c2dd928d551836c70c741ee836650 not backed up

Delete backup state file and try again
  $ rm .hg/commitcloud/backedupheads.*
  $ hg cloud check -r "draft()"
  cf2adfba146909529bcca8c1626de6b4d9e73846 backed up
  6c4f4b30ae4c2dd928d551836c70c741ee836650 not backed up

Back the commit up again
  $ hg cloud backup
  backing up stack rooted at 6c4f4b30ae4c
  remote: pushing 1 commit:
  remote:     6c4f4b30ae4c  headtwo
  commitcloud: backed up 1 commit

Hide the commit. Make sure isbackedup still works
  $ hg hide 6c4f4b30ae4c2dd928d551836c70c741ee836650
  hiding commit 6c4f4b30ae4c "headtwo"
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  working directory now at 630839011471
  1 changeset hidden
  $ hg cloud check -r 6c4f4b30ae4c2dd928d551836c70c741ee836650 --hidden
  6c4f4b30ae4c2dd928d551836c70c741ee836650 backed up

Test hostname option
  $ rm -r .hg/infinitepushbackups
  $ hg cloud backup --config infinitepushbackup.hostname=hostname
  nothing to back up
  $ scratchbookmarks | grep test/hostname
  infinitepush/backups/test/hostname$TESTTMP/client/bookmarks/bookonsecret dc80aa94cb8b16f962a5fb6e56e9ed234644b4e3
  infinitepush/backups/test/hostname$TESTTMP/client/bookmarks/somebook 630839011471e17f808b92ab084bedfaca33b110
  infinitepush/backups/test/hostname$TESTTMP/client/heads/cf2adfba146909529bcca8c1626de6b4d9e73846 cf2adfba146909529bcca8c1626de6b4d9e73846

Malformed backup state file
  $ echo rubbish > .hg/infinitepushbackups/infinitepushbackupstate*
  $ hg cloud backup
  corrupt file: infinitepushbackups/infinitepushbackupstate* (No JSON object could be decoded) (glob)
  nothing to back up

Run command that creates multiple transactions. Make sure that just one backup is started
  $ cd ..
  $ rm -rf client
  $ hg clone ssh://user@dummy/repo client -q
  $ cd client
  $ hg debugdrawdag <<'EOS'
  > C
  > |
  > B D
  > |/
  > A
  > EOS
  $ hg log -r ':' -G -T '{desc} {node}'
  o  C 26805aba1e600a82e93661149f2313866a221a7b
  |
  | o  D b18e25de2cf5fc4699a029ed635882849e53ef73
  | |
  o |  B 112478962961147124edd43549aedd1a335e44bf
  |/
  o  A 426bada5c67598ca65036d57d9e4b64b0c1ce7a0
  
  @  initial 630839011471e17f808b92ab084bedfaca33b110
  

Create logs directory and set correct permissions
  $ setuplogdir

  $ hg cloud backup --config infinitepushbackup.logdir=$TESTTMP/logs
  backing up stack rooted at 426bada5c675
  remote: pushing 4 commits:
  remote:     426bada5c675  A
  remote:     112478962961  B
  remote:     b18e25de2cf5  D
  remote:     26805aba1e60  C
  commitcloud: backed up 4 commits
  $ hg cloud check -r ':'
  630839011471e17f808b92ab084bedfaca33b110 not backed up
  426bada5c67598ca65036d57d9e4b64b0c1ce7a0 backed up
  112478962961147124edd43549aedd1a335e44bf backed up
  b18e25de2cf5fc4699a029ed635882849e53ef73 backed up
  26805aba1e600a82e93661149f2313866a221a7b backed up
  $ hg rebase -s B -d D --config infinitepushbackup.autobackup=True --config infinitepushbackup.logdir=$TESTTMP/logs
  rebasing 112478962961 "B" (B)
  rebasing 26805aba1e60 "C" (C)
  $ waitbgbackup
  $ hg log -r ':' -G -T '{desc} {node}'
  o  C ffeec75ec60331057b875fc5356c57c3ff204500
  |
  o  B 1ef11233b74dfa8b57e8285fd6f546096af8f4c2
  |
  o  D b18e25de2cf5fc4699a029ed635882849e53ef73
  |
  o  A 426bada5c67598ca65036d57d9e4b64b0c1ce7a0
  
  @  initial 630839011471e17f808b92ab084bedfaca33b110
  
  $ hg cloud check -r 'ffeec75ec + 1ef11233b7'
  ffeec75ec60331057b875fc5356c57c3ff204500 backed up
  1ef11233b74dfa8b57e8285fd6f546096af8f4c2 backed up

Check the logs, make sure just one process was started
  $ cat $TESTTMP/logs/test/*
  
  * starting: hg cloud backup --config 'ui.ssh=python "*/dummyssh" -bgssh' (glob)
  backing up stack rooted at 426bada5c675
  remote: pushing 4 commits:
  remote:     426bada5c675  A
  remote:     b18e25de2cf5  D
  remote:     1ef11233b74d  B
  remote:     ffeec75ec603  C
  commitcloud: backed up 2 commits

Check if ssh batch mode enables only for background backup and not for foreground
  $ mkcommit ssh1
  $ hg cloud backup --debug 2>&1 | debugsshcall
  running .* ".*/dummyssh" 'user@dummy' 'hg -R repo serve --stdio' (re)
  $ mkcommit ssh2
  $ hg cloud backup --background --config infinitepushbackup.logdir=$TESTTMP/logs --config infinitepushbackup.bgdebug=yes
  $ waitbgbackup
  $ cat $TESTTMP/logs/test/* | debugsshcall
  running .* ".*/dummyssh" -bgssh 'user@dummy' 'hg -R repo serve --stdio' (re)

Fail to push a backup by setting the server maxbundlesize very low
  $ cp ../repo/.hg/hgrc $TESTTMP/server-hgrc.bak
  $ cat >> ../repo/.hg/hgrc << EOF
  > [infinitepush]
  > maxbundlesize = 0
  > EOF
  $ mkcommit toobig
  $ hg cloud backup
  backing up stack rooted at acf5bae70f50
  remote: pushing 3 commits:
  remote:     acf5bae70f50  ssh1
  remote:     cb352c98cec7  ssh2
  remote:     b226c8ca23a2  toobig
  push failed: bundle is too big: 1488 bytes. max allowed size is 0 MB
  retrying push with discovery
  searching for changes
  remote: pushing 3 commits:
  remote:     acf5bae70f50  ssh1
  remote:     cb352c98cec7  ssh2
  remote:     b226c8ca23a2  toobig
  push of head b226c8ca23a2 failed: bundle is too big: 1488 bytes. max allowed size is 0 MB
  commitcloud: failed to back up 1 commit
  [2]
  $ hg cloud check -r .
  b226c8ca23a2db9b70a50978c6d30658683d9e9f not backed up
  $ scratchnodes | grep 034e9a5a003f9f7dd44ab4b35187e833d0aad5c3
  [1]

Set the limit back high, and try again
  $ mv $TESTTMP/server-hgrc.bak ../repo/.hg/hgrc
  $ hg cloud backup
  backing up stack rooted at acf5bae70f50
  remote: pushing 3 commits:
  remote:     acf5bae70f50  ssh1
  remote:     cb352c98cec7  ssh2
  remote:     b226c8ca23a2  toobig
  commitcloud: backed up 1 commit
  $ hg cloud check -r .
  b226c8ca23a2db9b70a50978c6d30658683d9e9f backed up
  $ scratchnodes | grep b226c8ca23a2db9b70a50978c6d30658683d9e9f
  b226c8ca23a2db9b70a50978c6d30658683d9e9f 17b31b46303cad87ba9dc939fcd19ce0f31c6df8

Remove the backup state file
  $ rm .hg/commitcloud/backedupheads.f6bce706

Remote check still succeeds
  $ hg cloud check -r . --remote
  b226c8ca23a2db9b70a50978c6d30658683d9e9f backed up

Local check should recover the file
  $ hg cloud check -r .
  b226c8ca23a2db9b70a50978c6d30658683d9e9f backed up
