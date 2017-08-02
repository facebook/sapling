
  $ setup() {
  > cat << EOF >> .hg/hgrc
  > [extensions]
  > fbamend=$TESTDIR/../hgext3rd/fbamend
  > [experimental]
  > evolution=createmarkers
  > EOF
  > }
  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/library-infinitepush.sh"
  $ setupcommon

Setup server
  $ hg init repo
  $ cd repo
  $ setupserver
  $ cd ..

Backup empty repo
  $ hg clone ssh://user@dummy/repo client -q
  $ cd client
  $ setup
  $ hg pushbackup
  starting backup .* (re)
  finished in \d+\.(\d+)? seconds (re)
  $ mkcommit commit
  $ hg prune .
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  working directory now at 000000000000
  1 changesets pruned
  $ mkcommit newcommit
  $ hg pushbackup
  starting backup .* (re)
  searching for changes
  remote: pushing 1 commit:
  remote:     606a357e69ad  newcommit
  finished in \d+\.(\d+)? seconds (re)

Re-clone the client
  $ cd ..
  $ rm -rf client
  $ hg clone ssh://user@dummy/repo client -q
  $ cd client

Setup client
  $ setup

Make commit and backup it. Use lockfail.py to make sure lock is not taken during
pushbackup
  $ mkcommit commit
  $ hg pushbackup --config extensions.lockfail=$TESTDIR/lockfail.py
  starting backup .* (re)
  searching for changes
  remote: pushing 1 commit:
  remote:     7e6a6fd9c7c8  commit
  finished in \d+\.(\d+)? seconds (re)
  $ scratchnodes
  606a357e69adb2e36d559ae3237626e82a955c9d 9fa7f02468b18919035248ab21c8267674c0a3d6
  7e6a6fd9c7c8c8c307ee14678f03d63af3a7b455 168423c30397d95ef5f44d883f0887f0f5be0936
  $ scratchbookmarks
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/7e6a6fd9c7c8c8c307ee14678f03d63af3a7b455 7e6a6fd9c7c8c8c307ee14678f03d63af3a7b455 (re)

Make first commit public (by doing push) and then backup new commit
  $ hg push
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files
  $ mkcommit newcommit
  $ hg pushbackup
  starting backup .* (re)
  searching for changes
  remote: pushing 1 commit:
  remote:     94a60f5ad8b2  newcommit
  finished in \d+\.(\d+)? seconds (re)
  $ scratchbookmarks
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/94a60f5ad8b2e007240007edab982b3638a3f38d 94a60f5ad8b2e007240007edab982b3638a3f38d (re)

Create obsoleted commit
  $ mkcommit obsoletedcommit
  $ hg prune .
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  working directory now at 94a60f5ad8b2
  1 changesets pruned

Make obsoleted commit non-extinct by committing on top of it
  $ hg --hidden up 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ mkcommit ontopofobsoleted

Backup both of them
  $ hg pushbackup
  starting backup .* (re)
  searching for changes
  remote: pushing 3 commits:
  remote:     94a60f5ad8b2  newcommit
  remote:     361e89f06232  obsoletedcommit
  remote:     d5609f7fa633  ontopofobsoleted
  finished in \d+\.(\d+)? seconds (re)
  $ scratchbookmarks
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/d5609f7fa63352da538eeffbe3ffabed1779aafc d5609f7fa63352da538eeffbe3ffabed1779aafc (re)

Create one more head and run `hg pushbackup`. Make sure that only new head is pushed
  $ hg up 0
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved
  $ mkcommit newhead
  created new head
  $ hg pushbackup
  starting backup .* (re)
  searching for changes
  remote: pushing 1 commit:
  remote:     3a30e220fe42  newhead
  finished in \d+\.(\d+)? seconds (re)

Create two more heads and backup them
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit newhead1
  created new head
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit newhead2
  created new head
  $ hg pushbackup
  starting backup .* (re)
  searching for changes
  remote: pushing 2 commits:
  remote:     f79c5017def3  newhead1
  remote:     667453c0787e  newhead2
  finished in \d+\.(\d+)? seconds (re)

Backup in background
  $ scratchbookmarks
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/3a30e220fe42e969e34bbe8001b951a20f31f2e8 3a30e220fe42e969e34bbe8001b951a20f31f2e8 (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/667453c0787e7830fdfb86db0f8c29aa7af2a1ea 667453c0787e7830fdfb86db0f8c29aa7af2a1ea (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/d5609f7fa63352da538eeffbe3ffabed1779aafc d5609f7fa63352da538eeffbe3ffabed1779aafc (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/f79c5017def3b9af9928edbb52cc620c74b4b291 f79c5017def3b9af9928edbb52cc620c74b4b291 (re)
  $ mkcommit newcommit
  $ hg pushbackup --background
  $ waitbgbackup
  $ scratchbookmarks
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/3a30e220fe42e969e34bbe8001b951a20f31f2e8 3a30e220fe42e969e34bbe8001b951a20f31f2e8 (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/773a3ba2e7c25358df2e5b3cced70371333bc61c 773a3ba2e7c25358df2e5b3cced70371333bc61c (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/d5609f7fa63352da538eeffbe3ffabed1779aafc d5609f7fa63352da538eeffbe3ffabed1779aafc (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/f79c5017def3b9af9928edbb52cc620c74b4b291 f79c5017def3b9af9928edbb52cc620c74b4b291 (re)

Backup with bookmark
  $ mkcommit commitwithbookmark
  $ hg book abook
  $ hg pushbackup
  starting backup .* (re)
  searching for changes
  remote: pushing 3 commits:
  remote:     667453c0787e  newhead2
  remote:     773a3ba2e7c2  newcommit
  remote:     166ff4468f7d  commitwithbookmark
  finished in \d+\.(\d+)? seconds (re)
  $ scratchbookmarks
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/bookmarks/abook 166ff4468f7da443df90d268158ba7d75d52585a (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/166ff4468f7da443df90d268158ba7d75d52585a 166ff4468f7da443df90d268158ba7d75d52585a (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/3a30e220fe42e969e34bbe8001b951a20f31f2e8 3a30e220fe42e969e34bbe8001b951a20f31f2e8 (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/d5609f7fa63352da538eeffbe3ffabed1779aafc d5609f7fa63352da538eeffbe3ffabed1779aafc (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/f79c5017def3b9af9928edbb52cc620c74b4b291 f79c5017def3b9af9928edbb52cc620c74b4b291 (re)

Backup only bookmarks
  $ hg book newbook
  $ hg pushbackup
  starting backup .* (re)
  finished in \d+\.(\d+)? seconds (re)
  $ scratchbookmarks
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/bookmarks/abook 166ff4468f7da443df90d268158ba7d75d52585a (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/bookmarks/newbook 166ff4468f7da443df90d268158ba7d75d52585a (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/166ff4468f7da443df90d268158ba7d75d52585a 166ff4468f7da443df90d268158ba7d75d52585a (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/3a30e220fe42e969e34bbe8001b951a20f31f2e8 3a30e220fe42e969e34bbe8001b951a20f31f2e8 (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/d5609f7fa63352da538eeffbe3ffabed1779aafc d5609f7fa63352da538eeffbe3ffabed1779aafc (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/f79c5017def3b9af9928edbb52cc620c74b4b291 f79c5017def3b9af9928edbb52cc620c74b4b291 (re)

Nothing changed, make sure no backup happens
  $ hg pushbackup
  starting backup .* (re)
  nothing to backup
  finished in \d+\.(\d+)? seconds (re)

Obsolete a head, make sure backup happens
  $ hg prune .
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  working directory now at 773a3ba2e7c2
  1 changesets pruned
  $ hg pushbackup
  starting backup .* (re)
  searching for changes
  remote: pushing 2 commits:
  remote:     667453c0787e  newhead2
  remote:     773a3ba2e7c2  newcommit
  finished in \d+\.(\d+)? seconds (re)
  $ scratchbookmarks
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/bookmarks/abook 773a3ba2e7c25358df2e5b3cced70371333bc61c (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/bookmarks/newbook 773a3ba2e7c25358df2e5b3cced70371333bc61c (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/3a30e220fe42e969e34bbe8001b951a20f31f2e8 3a30e220fe42e969e34bbe8001b951a20f31f2e8 (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/773a3ba2e7c25358df2e5b3cced70371333bc61c 773a3ba2e7c25358df2e5b3cced70371333bc61c (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/d5609f7fa63352da538eeffbe3ffabed1779aafc d5609f7fa63352da538eeffbe3ffabed1779aafc (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/f79c5017def3b9af9928edbb52cc620c74b4b291 f79c5017def3b9af9928edbb52cc620c74b4b291 (re)

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
  rebasing 5:f79c5017def3 "newhead1"
  $ hg pushbackup
  starting backup .* (re)
  searching for changes
  remote: pushing 3 commits:
  remote:     667453c0787e  newhead2
  remote:     773a3ba2e7c2  newcommit
  remote:     2d2e01441947  newhead1
  finished in \d+\.(\d+)? seconds (re)
  $ scratchbookmarks
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/bookmarks/abook 773a3ba2e7c25358df2e5b3cced70371333bc61c (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/bookmarks/newbook 773a3ba2e7c25358df2e5b3cced70371333bc61c (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/2d2e01441947afbb6bb5ae0efbb901f3eebe3fbd 2d2e01441947afbb6bb5ae0efbb901f3eebe3fbd (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/3a30e220fe42e969e34bbe8001b951a20f31f2e8 3a30e220fe42e969e34bbe8001b951a20f31f2e8 (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/d5609f7fa63352da538eeffbe3ffabed1779aafc d5609f7fa63352da538eeffbe3ffabed1779aafc (re)
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
  
  $ hg pushbackup
  starting backup .* (re)
  finished in \d+\.(\d+)? seconds (re)
  $ scratchbookmarks
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/bookmarks/abook 773a3ba2e7c25358df2e5b3cced70371333bc61c (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/bookmarks/newbook 773a3ba2e7c25358df2e5b3cced70371333bc61c (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/3a30e220fe42e969e34bbe8001b951a20f31f2e8 3a30e220fe42e969e34bbe8001b951a20f31f2e8 (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/d5609f7fa63352da538eeffbe3ffabed1779aafc d5609f7fa63352da538eeffbe3ffabed1779aafc (re)
  $ hg pushbackup
  starting backup .* (re)
  nothing to backup
  finished in \d+\.(\d+)? seconds (re)

Backup bookmark that has '/bookmarks/' in the name. Make sure it was escaped
  $ hg book new/bookmarks/book
  $ hg pushbackup
  starting backup .* (re)
  finished in \d+\.(\d+)? seconds (re)
  $ scratchbookmarks
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/bookmarks/abook 773a3ba2e7c25358df2e5b3cced70371333bc61c (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/bookmarks/new/bookmarksbookmarks/book 3446a384dd701da41cd83cbd9562805fc6412c0e (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/bookmarks/newbook 773a3ba2e7c25358df2e5b3cced70371333bc61c (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/3a30e220fe42e969e34bbe8001b951a20f31f2e8 3a30e220fe42e969e34bbe8001b951a20f31f2e8 (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/d5609f7fa63352da538eeffbe3ffabed1779aafc d5609f7fa63352da538eeffbe3ffabed1779aafc (re)

Backup to different path
  $ cat >> .hg/hgrc << EOF
  > [paths]
  > default = brokenpath
  > nondefault = ssh://user@dummy/repo
  > EOF
  $ hg book somebook
  $ hg --config paths.default=brokenpath pushbackup
  starting backup .* (re)
  abort: repository $TESTTMP/client/brokenpath not found!
  [255]
  $ hg pushbackup nondefault --traceback
  starting backup .* (re)
  finished in \d+\.(\d+)? seconds (re)
  $ scratchbookmarks
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/bookmarks/abook 773a3ba2e7c25358df2e5b3cced70371333bc61c (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/bookmarks/new/bookmarksbookmarks/book 3446a384dd701da41cd83cbd9562805fc6412c0e (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/bookmarks/newbook 773a3ba2e7c25358df2e5b3cced70371333bc61c (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/bookmarks/somebook 3446a384dd701da41cd83cbd9562805fc6412c0e (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/3a30e220fe42e969e34bbe8001b951a20f31f2e8 3a30e220fe42e969e34bbe8001b951a20f31f2e8 (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/d5609f7fa63352da538eeffbe3ffabed1779aafc d5609f7fa63352da538eeffbe3ffabed1779aafc (re)

Backup in background to different path
  $ mkcommit backgroundcommittodifferentpath
  $ hg pushbackup nondefault --background
  $ waitbgbackup
  $ scratchbookmarks
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/bookmarks/abook 773a3ba2e7c25358df2e5b3cced70371333bc61c (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/bookmarks/new/bookmarksbookmarks/book 3446a384dd701da41cd83cbd9562805fc6412c0e (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/bookmarks/newbook 773a3ba2e7c25358df2e5b3cced70371333bc61c (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/bookmarks/somebook 268f86e364f9aed2f5bb9d11e2df6381ace129a2 (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/268f86e364f9aed2f5bb9d11e2df6381ace129a2 268f86e364f9aed2f5bb9d11e2df6381ace129a2 (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/3a30e220fe42e969e34bbe8001b951a20f31f2e8 3a30e220fe42e969e34bbe8001b951a20f31f2e8 (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/d5609f7fa63352da538eeffbe3ffabed1779aafc d5609f7fa63352da538eeffbe3ffabed1779aafc (re)

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
  $ setup

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
Make sure pushbackup works
  $ mkcommit toobsolete
  $ hg prune .
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  working directory now at 630839011471
  1 changesets pruned
  $ hg book somebook
  $ hg pushbackup
  starting backup .* (re)
  finished in \d+\.(\d+)? seconds (re)
  $ scratchbookmarks
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/bookmarks/somebook 630839011471e17f808b92ab084bedfaca33b110 (re)

Make secret commit and bookmark on top of it. Then run pushbackup.
Make sure it wasn't backed up.
  $ hg book bookonsecret
  $ echo secret >> secret
  $ hg add secret
  $ hg ci -Am secret --secret
  $ hg pushbackup
  starting backup .* (re)
  nothing to backup
  finished in \d+\.(\d+)? seconds (re)
  $ scratchbookmarks
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/bookmarks/somebook 630839011471e17f808b92ab084bedfaca33b110 (re)

Create two heads, set maxheadstobackup to 1, make sure only latest head was backed up
  $ hg up -q 0
  $ mkcommit headone
  created new head
  $ hg up -q 0
  $ mkcommit headtwo
  created new head
  $ hg pushbackup --config infinitepushbackup.maxheadstobackup=1
  starting backup .* (re)
  searching for changes
  remote: pushing 1 commit:
  remote:     6c4f4b30ae4c  headtwo
  finished in \d+\.(\d+)? seconds (re)
  $ scratchbookmarks
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/bookmarks/somebook 630839011471e17f808b92ab084bedfaca33b110 (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/6c4f4b30ae4c2dd928d551836c70c741ee836650 6c4f4b30ae4c2dd928d551836c70c741ee836650 (re)

Now set maxheadstobackup to 0 and backup again. Make sure nothing is backed up now
  $ hg pushbackup --config infinitepushbackup.maxheadstobackup=0
  starting backup .* (re)
  finished in \d+\.(\d+)? seconds (re)
  $ scratchbookmarks
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/bookmarks/somebook 630839011471e17f808b92ab084bedfaca33b110 (re)

Test isbackedup command
  $ hg pushbackup
  starting backup .* (re)
  searching for changes
  remote: pushing 2 commits:
  remote:     cf2adfba1469  headone
  remote:     6c4f4b30ae4c  headtwo
  finished in \d+\.(\d+)? seconds (re)
  $ hg isbackedup
  6c4f4b30ae4c2dd928d551836c70c741ee836650 backed up
  $ hg isbackedup -r 630839011471e17
  630839011471e17f808b92ab084bedfaca33b110 not backed up
  $ hg isbackedup -r . -r 630839011471e17
  6c4f4b30ae4c2dd928d551836c70c741ee836650 backed up
  630839011471e17f808b92ab084bedfaca33b110 not backed up

Delete backup state file and try again
  $ rm .hg/infinitepushbackupstate
  $ hg isbackedup -r . -r 630839011471e17
  6c4f4b30ae4c2dd928d551836c70c741ee836650 not backed up
  630839011471e17f808b92ab084bedfaca33b110 not backed up

Prune commit and then inhibit obsmarkers. Make sure isbackedup still works
  $ hg pushbackup
  starting backup .* (re)
  searching for changes
  remote: pushing 2 commits:
  remote:     cf2adfba1469  headone
  remote:     6c4f4b30ae4c  headtwo
  finished in \d+\.(\d+)? seconds (re)
  $ hg prune 6c4f4b30ae4c2dd928d551836c70c741ee836650
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  working directory now at 630839011471
  1 changesets pruned
  $ hg isbackedup -r 6c4f4b30ae4c2dd928d551836c70c741ee836650
  6c4f4b30ae4c2dd928d551836c70c741ee836650 backed up

Test backupgeneration config option. If this config option value changes then
new full backup should be made.
  $ hg pushbackup
  starting backup .* (re)
  finished in \d+\.(\d+)? seconds (re)
  $ hg pushbackup --config infinitepushbackup.backupgeneration=1
  starting backup .* (re)
  searching for changes
  remote: pushing 1 commit:
  remote:     cf2adfba1469  headone
  finished in \d+\.(\d+)? seconds (re)

Next backup with the same backup generation value should not trigger full backup
  $ hg pushbackup --config infinitepushbackup.backupgeneration=1
  starting backup .* (re)
  nothing to backup
  finished in \d+\.(\d+)? seconds (re)
  $ cat .hg/infinitepushbackupgeneration
  1 (no-eol)

Print garbage to infinitepushbackupgeneration file, make sure backup works fine
  $ echo 'garbage' > .hg/infinitepushbackupgeneration
  $ hg pushbackup
  starting backup .* (re)
  nothing to backup
  finished in \d+\.(\d+)? seconds (re)

Delete infinitepushbackupstate and set backupgeneration. Make sure it doesn't fail
  $ rm .hg/infinitepushbackupstate
  $ hg pushbackup --config infinitepushbackup.backupgeneration=2
  starting backup * (glob)
  searching for changes
  remote: pushing 1 commit:
  remote:     cf2adfba1469  headone
  finished in * seconds (glob)

Test hostname option
  $ rm .hg/infinitepushbackupstate
  $ hg pushbackup --config infinitepushbackup.hostname=hostname
  starting backup * (glob)
  searching for changes
  remote: pushing 1 commit:
  remote:     cf2adfba1469  headone
  finished in \d+\.(\d+)? seconds (re)
  $ scratchbookmarks | grep test/hostname
  infinitepush/backups/test/hostname$TESTTMP/client/bookmarks/somebook 630839011471e17f808b92ab084bedfaca33b110
  infinitepush/backups/test/hostname$TESTTMP/client/heads/cf2adfba146909529bcca8c1626de6b4d9e73846 cf2adfba146909529bcca8c1626de6b4d9e73846

Malformed backup state file
  $ echo rubbish > .hg/infinitepushbackupstate
  $ hg pushbackup
  starting backup * (glob)
  corrupt file: infinitepushbackupstate (No JSON object could be decoded)
  searching for changes
  remote: pushing 1 commit:
  remote:     cf2adfba1469  headone
  finished in * seconds (glob)

Test backupinfo file generation.
  $ [ -f .hg/infinitepushlatestbackupinfo ]
  [1]
  $ rm .hg/infinitepushbackupstate
  $ hg pushbackup --config infinitepushbackup.savelatestbackupinfo=True
  starting backup .* (re)
  searching for changes
  remote: pushing 1 commit:
  remote:     cf2adfba1469  headone
  finished in \d+\.(\d+)? seconds (re)
  $ cat .hg/infinitepushlatestbackupinfo
  backuprevision=(\d+) (re)
  backuptime=(\d+) (re)
