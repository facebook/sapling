#chg-compatible

  $ enable commitcloud infinitepush amend rebase remotenames
  $ setconfig ui.ssh="python \"$TESTDIR/dummyssh\""
  $ setconfig commitcloud.hostname=testhost
  $ setconfig remotefilelog.reponame=testrepo
  $ setconfig mutation.record=true mutation.enabled=true
  $ setconfig extensions.treemanifest=!
  $ setconfig experimental.narrow-heads=true
  $ setconfig visibility.enabled=true

  $ newrepo server
  $ setconfig infinitepush.server=yes infinitepush.reponame=testrepo
  $ setconfig infinitepush.indextype=disk infinitepush.storetype=disk
  $ setconfig treemanifest.server=True
  $ touch base
  $ hg commit -Aqm base
  $ hg phase -p .
  (phases are now managed by remotenames and heads; manully editing phases is a no-op)
  $ hg bookmark master
  $ cd ..

  $ hg clone ssh://user@dummy/server client1 -q
  $ cd client1
  $ setconfig commitcloud.servicetype=local
  $ setconfig commitcloud.servicelocation="$TESTTMP"
  $ setconfig commitcloud.user_token_path="$TESTTMP"
  $ setconfig extensions.extralog="$TESTDIR/extralog.py"
  $ setconfig extralog.events="visibility, commitcloud_sync"
  $ setconfig extensions.lockdelay="$TESTDIR/lockdelay.py"
  $ hg cloud auth -t XXXXXX
  setting authentication token
  authentication successful
  $ hg cloud join
  commitcloud: this repository is now connected to the 'user/test/default' workspace for the 'testrepo' repo
  visibility: read 1 heads: df4f53cec30a
  commitcloud: synchronizing 'testrepo' with 'user/test/default'
  commitcloud_sync: synced to workspace user/test/default version 1: 1 heads (0 omitted), 0 bookmarks (0 omitted), 0 remote bookmarks, 0 snapshots
  commitcloud: commits synchronized
  finished in 0.00 sec
  $ cd ..

  $ hg clone ssh://user@dummy/server client2 -q
  $ cd client2
  $ setconfig commitcloud.servicetype=local
  $ setconfig commitcloud.servicelocation="$TESTTMP"
  $ setconfig commitcloud.user_token_path="$TESTTMP"
  $ setconfig extensions.extralog="$TESTDIR/extralog.py"
  $ setconfig extralog.events="visibility, commitcloud_sync"
  $ setconfig extensions.lockdelay="$TESTDIR/lockdelay.py"
  $ hg cloud auth -t XXXXXX
  updating authentication token
  authentication successful
  $ hg cloud join
  commitcloud: this repository is now connected to the 'user/test/default' workspace for the 'testrepo' repo
  visibility: read 1 heads: df4f53cec30a
  commitcloud: synchronizing 'testrepo' with 'user/test/default'
  commitcloud_sync: synced to workspace user/test/default version 1: 1 heads (0 omitted), 0 bookmarks (0 omitted), 0 remote bookmarks, 0 snapshots
  commitcloud: commits synchronized
  finished in 0.00 sec
  $ cd ..

  $ cd client1
  $ touch 1
  $ hg commit -Aqm commit1
  visibility: read 1 heads: df4f53cec30a
  visibility: removed 1 heads [df4f53cec30a]; added 1 heads [79089e97b9e7]
  visibility: wrote 1 heads: 79089e97b9e7
  $ hg cloud sync
  visibility: read 1 heads: 79089e97b9e7
  commitcloud: synchronizing 'testrepo' with 'user/test/default'
  backing up stack rooted at 79089e97b9e7
  remote: pushing 1 commit:
  remote:     79089e97b9e7  commit1
  commitcloud_sync: synced to workspace user/test/default version 2: 1 heads (0 omitted), 0 bookmarks (0 omitted), 0 remote bookmarks, 0 snapshots
  commitcloud: commits synchronized
  finished in 0.00 sec

  $ cd ../client2

Start a background sync to pull in the changes from the other repo.

  $ touch $TESTTMP/wlockpre1
  $ HGPREWLOCKFILE=$TESTTMP/wlockpre1 hg cloud sync > $TESTTMP/bgsync.out 2>&1 &

While that is getting started, create a new commit locally.

  $ sleep 1
  $ touch 2
  $ hg commit -Aqm commit2
  visibility: read 1 heads: df4f53cec30a
  visibility: removed 1 heads [df4f53cec30a]; added 1 heads [1292cc1f1c17]
  visibility: wrote 1 heads: 1292cc1f1c17
  $ hg up -q 0
  visibility: read 1 heads: 1292cc1f1c17
  $ tglogp
  visibility: read 1 heads: 1292cc1f1c17
  o  1: 1292cc1f1c17 draft 'commit2'
  |
  @  0: df4f53cec30a public 'base'
  

Let the background sync we started earlier continue, and start a concurrent cloud sync.

  $ rm $TESTTMP/wlockpre1
  $ hg cloud sync
  visibility: read 1 heads: 1292cc1f1c17
  commitcloud: synchronizing 'testrepo' with 'user/test/default'
  backing up stack rooted at 1292cc1f1c17
  remote: pushing 1 commit:
  remote:     1292cc1f1c17  commit2
  pulling 79089e97b9e7
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  visibility: removed 0 heads []; added 1 heads [79089e97b9e7]
  commitcloud_sync: synced to workspace user/test/default version 2: 1 heads (0 omitted), 0 bookmarks (0 omitted), 0 remote bookmarks, 0 snapshots
  commitcloud_sync: synced to workspace user/test/default version 3: 2 heads (0 omitted), 0 bookmarks (0 omitted), 0 remote bookmarks, 0 snapshots
  visibility: wrote 2 heads: 79089e97b9e7, 1292cc1f1c17
  new changesets 79089e97b9e7
  commitcloud: commits synchronized
  finished in 0.00 sec

  $ tglogp
  visibility: read 2 heads: 79089e97b9e7, 1292cc1f1c17
  o  2: 79089e97b9e7 draft 'commit1'
  |
  | o  1: 1292cc1f1c17 draft 'commit2'
  |/
  @  0: df4f53cec30a public 'base'
  
Wait for the background backup to finish and check its output.

  $ hg debugwaitbackup
  $ cat $TESTTMP/bgsync.out
  visibility: read 1 heads: df4f53cec30a
  commitcloud: synchronizing 'testrepo' with 'user/test/default'
  visibility: read 1 heads: 1292cc1f1c17
  abort: commitcloud: failed to synchronize commits: 'repo changed while backing up'
  (please retry 'hg cloud sync')
  (please contact the Source Control Team if this error persists)
