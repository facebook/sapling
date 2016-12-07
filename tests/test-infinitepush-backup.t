
  $ . $TESTDIR/require-ext.sh evolve
  $ setupevolve() {
  > cat << EOF >> .hg/hgrc
  > [extensions]
  > evolve=
  > [experimental]
  > evolution=createmarkers
  > evolutioncommands=obsolete
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
  $ setupevolve
  $ hg debugbackup
  nothing to backup
  $ mkcommit commit
  $ hg prune .
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  working directory now at 000000000000
  1 changesets pruned
  $ mkcommit newcommit
  $ hg debugbackup
  searching for changes
  remote: pushing 1 commit:
  remote:     606a357e69ad  newcommit

Re-clone the client
  $ cd ..
  $ rm -rf client
  $ hg clone ssh://user@dummy/repo client -q
  $ cd client

Setup client
  $ setupevolve

Make commit and backup it. Use lockfail.py to make sure lock is not taken during
debugbackup
  $ mkcommit commit
  $ hg debugbackup --config extensions.lockfail=$TESTDIR/lockfail.py
  searching for changes
  remote: pushing 1 commit:
  remote:     7e6a6fd9c7c8  commit
  $ scratchnodes
  606a357e69adb2e36d559ae3237626e82a955c9d 2a40dbb1b839c5720ba2662ca1329373674a6e3a
  7e6a6fd9c7c8c8c307ee14678f03d63af3a7b455 6ff0097c5e81598752887c6990977a3a1f981004
  $ scratchbookmarks
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/7e6a6fd9c7c8c8c307ee14678f03d63af3a7b455 7e6a6fd9c7c8c8c307ee14678f03d63af3a7b455 (re)
  $ cat .hg/store/infinitepushlastbackupedstate
  0 [0-9a-f]{40} \(no-eol\) (re)

Make first commit public (by doing push) and then backup new commit
  $ hg push
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files
  $ mkcommit newcommit
  $ hg debugbackup
  searching for changes
  remote: pushing 1 commit:
  remote:     94a60f5ad8b2  newcommit
  $ scratchbookmarks
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/94a60f5ad8b2e007240007edab982b3638a3f38d 94a60f5ad8b2e007240007edab982b3638a3f38d (re)
  $ cat .hg/store/infinitepushlastbackupedstate
  1 [0-9a-f]{40} \(no-eol\) (re)

Create obsoleted commit
  $ mkcommit obsoletedcommit
  $ hg prune .
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  working directory now at 94a60f5ad8b2
  1 changesets pruned

Make obsoleted commit non-extinct by committing on top of it
  $ hg --hidden up 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  working directory parent is obsolete!
  $ mkcommit ontopofobsoleted
  1 new unstable changesets

Backup both of them
  $ hg debugbackup
  searching for changes
  remote: pushing 3 commits:
  remote:     94a60f5ad8b2  newcommit
  remote:     361e89f06232  obsoletedcommit
  remote:     d5609f7fa633  ontopofobsoleted
  $ cat .hg/store/infinitepushlastbackupedstate
  3 [0-9a-f]{40} \(no-eol\) (re)

Create one more head and run `hg debugbackup`. Make sure that only new head is pushed
  $ hg up 0
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved
  $ mkcommit newhead
  created new head
  $ hg debugbackup
  searching for changes
  remote: pushing 1 commit:
  remote:     3a30e220fe42  newhead

Create two more heads and backup them
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit newhead1
  created new head
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit newhead2
  created new head
  $ hg debugbackup
  searching for changes
  remote: pushing 2 commits:
  remote:     f79c5017def3  newhead1
  remote:     667453c0787e  newhead2

Backup in background
  $ cat .hg/store/infinitepushlastbackupedstate
  6 [0-9a-f]{40} \(no-eol\) (re)
  $ mkcommit newcommit
  $ tip=`hg log -r tip -T '{rev}'`
  $ hg --config infinitepush.debugbackuplog="$TESTTMP/logfile" debugbackup --background
  >>> from time import sleep
  >>> for i in range(100):
  ...   sleep(0.1)
  ...   backuptip = int(open('.hg/store/infinitepushlastbackupedstate').read().split(' ')[0])
  ...   if backuptip == 7:
  ...     break
  $ cat $TESTTMP/logfile
  searching for changes
  remote: pushing 2 commits:
  remote:     667453c0787e  newhead2
  remote:     773a3ba2e7c2  newcommit
  $ cat .hg/store/infinitepushlastbackupedstate
  7 [0-9a-f]{40} \(no-eol\) (re)

Backup with bookmark
  $ mkcommit commitwithbookmark
  $ hg book abook
  $ hg debugbackup
  searching for changes
  remote: pushing 3 commits:
  remote:     667453c0787e  newhead2
  remote:     773a3ba2e7c2  newcommit
  remote:     166ff4468f7d  commitwithbookmark
  $ scratchbookmarks
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/bookmarks/abook 166ff4468f7da443df90d268158ba7d75d52585a (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/166ff4468f7da443df90d268158ba7d75d52585a 166ff4468f7da443df90d268158ba7d75d52585a (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/3a30e220fe42e969e34bbe8001b951a20f31f2e8 3a30e220fe42e969e34bbe8001b951a20f31f2e8 (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/d5609f7fa63352da538eeffbe3ffabed1779aafc d5609f7fa63352da538eeffbe3ffabed1779aafc (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/f79c5017def3b9af9928edbb52cc620c74b4b291 f79c5017def3b9af9928edbb52cc620c74b4b291 (re)

Backup only bookmarks
  $ hg book newbook
  $ hg debugbackup
  $ scratchbookmarks
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/bookmarks/abook 166ff4468f7da443df90d268158ba7d75d52585a (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/bookmarks/newbook 166ff4468f7da443df90d268158ba7d75d52585a (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/166ff4468f7da443df90d268158ba7d75d52585a 166ff4468f7da443df90d268158ba7d75d52585a (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/3a30e220fe42e969e34bbe8001b951a20f31f2e8 3a30e220fe42e969e34bbe8001b951a20f31f2e8 (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/d5609f7fa63352da538eeffbe3ffabed1779aafc d5609f7fa63352da538eeffbe3ffabed1779aafc (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/f79c5017def3b9af9928edbb52cc620c74b4b291 f79c5017def3b9af9928edbb52cc620c74b4b291 (re)

Nothing changed, make sure no backup happens
  $ hg debugbackup
  nothing to backup

Obsolete a head, make sure backup happens
  $ hg prune .
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  working directory now at 773a3ba2e7c2
  1 changesets pruned
  $ hg debugbackup
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
  $ hg debugbackup
  searching for changes
  remote: pushing 3 commits:
  remote:     667453c0787e  newhead2
  remote:     773a3ba2e7c2  newcommit
  remote:     2d2e01441947  newhead1
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
  
  $ hg debugbackup
  $ scratchbookmarks
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/bookmarks/abook 773a3ba2e7c25358df2e5b3cced70371333bc61c (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/bookmarks/newbook 773a3ba2e7c25358df2e5b3cced70371333bc61c (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/3a30e220fe42e969e34bbe8001b951a20f31f2e8 3a30e220fe42e969e34bbe8001b951a20f31f2e8 (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/d5609f7fa63352da538eeffbe3ffabed1779aafc d5609f7fa63352da538eeffbe3ffabed1779aafc (re)
  $ hg debugbackup
  nothing to backup

Backup bookmark that has '/bookmarks/' in the name. Make sure it was escaped
  $ hg book new/bookmarks/book
  $ hg debugbackup
  $ scratchbookmarks
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/bookmarks/abook 773a3ba2e7c25358df2e5b3cced70371333bc61c (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/bookmarks/new/bookmarksbookmarks/book 3446a384dd701da41cd83cbd9562805fc6412c0e (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/bookmarks/newbook 773a3ba2e7c25358df2e5b3cced70371333bc61c (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/3a30e220fe42e969e34bbe8001b951a20f31f2e8 3a30e220fe42e969e34bbe8001b951a20f31f2e8 (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/d5609f7fa63352da538eeffbe3ffabed1779aafc d5609f7fa63352da538eeffbe3ffabed1779aafc (re)
