#chg-compatible
#debugruntest-compatible
#inprocess-hg-incompatible
  $ setconfig workingcopy.ruststatus=False
  $ setconfig experimental.allowfilepeer=True

  $ enable commitcloud infinitepush amend rebase remotenames
  $ configure dummyssh
  $ setconfig commitcloud.hostname=testhost
  $ setconfig remotefilelog.reponame=testrepo
  $ setconfig mutation.record=true mutation.enabled=true
  $ setconfig experimental.narrow-heads=true
  $ setconfig visibility.enabled=true

  $ newrepo server --config extensions.treemanifest=$TESTDIR/../edenscm/hgext/treemanifestserver.py
  $ setconfig infinitepush.server=yes infinitepush.reponame=testrepo
  $ setconfig infinitepush.indextype=disk infinitepush.storetype=disk
  $ setconfig treemanifest.server=True extensions.treemanifest=$TESTDIR/../edenscm/hgext/treemanifestserver.py
  $ touch base
  $ hg commit -Aqm base
  $ hg bookmark master
  $ cd ..

  $ hg clone ssh://user@dummy/server client1 -q
  $ cd client1
  $ setconfig commitcloud.servicetype=local commitcloud.servicelocation=$TESTTMP commitcloud.token_enforced=False
  $ setconfig extensions.extralog="$TESTDIR/extralog.py"
  $ setconfig extralog.events="visibility, commitcloud_sync"
  $ setconfig extensions.lockdelay="$TESTDIR/lockdelay.py"
  $ hg cloud join
  commitcloud: this repository is now connected to the 'user/test/default' workspace for the 'testrepo' repo
  commitcloud: synchronizing 'testrepo' with 'user/test/default'
  visibility: read 0 heads: 
  commitcloud_sync: synced to workspace user/test/default version 1: 0 heads (0 omitted), 0 bookmarks (0 omitted), 0 remote bookmarks (0 omitted)
  commitcloud: commits synchronized
  finished in 0.00 sec
  $ cd ..

  $ hg clone ssh://user@dummy/server client2 -q
  $ cd client2
  $ setconfig commitcloud.servicetype=local commitcloud.servicelocation=$TESTTMP commitcloud.token_enforced=False
  $ setconfig extensions.extralog="$TESTDIR/extralog.py"
  $ setconfig extralog.events="visibility, commitcloud_sync"
  $ setconfig extensions.lockdelay="$TESTDIR/lockdelay.py"
  $ hg cloud join
  commitcloud: this repository is now connected to the 'user/test/default' workspace for the 'testrepo' repo
  commitcloud: synchronizing 'testrepo' with 'user/test/default'
  visibility: read 0 heads: 
  commitcloud_sync: synced to workspace user/test/default version 1: 0 heads (0 omitted), 0 bookmarks (0 omitted), 0 remote bookmarks (0 omitted)
  commitcloud: commits synchronized
  finished in 0.00 sec
  $ cd ..

  $ cd client1
  $ touch 1
  $ hg commit -Aqm commit1
  visibility: read 0 heads: 
  visibility: removed 0 heads []; added 1 heads [79089e97b9e7]
  visibility: wrote 1 heads: 79089e97b9e7
  $ hg cloud sync
  commitcloud: synchronizing 'testrepo' with 'user/test/default'
  visibility: read 1 heads: 79089e97b9e7
  backing up stack rooted at 79089e97b9e7
  commitcloud_sync: synced to workspace user/test/default version 2: 1 heads (0 omitted), 0 bookmarks (0 omitted), 0 remote bookmarks (0 omitted)
  commitcloud: commits synchronized
  finished in 0.00 sec
  remote: pushing 1 commit:
  remote:     79089e97b9e7  commit1

  $ cd ../client2

Start a background sync to pull in the changes from the other repo.

  $ touch $TESTTMP/wlockpre1
  $ HGPREWLOCKFILE=$TESTTMP/wlockpre1 hg cloud sync > $TESTTMP/bgsync.out 2>&1 &

While that is getting started, create a new commit locally.

  $ sleep 1
  $ touch 2
  $ hg commit -Aqm commit2
  visibility: read 0 heads: 
  visibility: removed 0 heads []; added 1 heads [1292cc1f1c17]
  visibility: wrote 1 heads: 1292cc1f1c17
  $ hg up -q 'desc(base)'
  visibility: read 1 heads: 1292cc1f1c17
  $ tglogp
  visibility: read 1 heads: 1292cc1f1c17
  o  1292cc1f1c17 draft 'commit2'
  │
  @  df4f53cec30a public 'base'
  

Let the background sync we started earlier continue, and start a concurrent cloud sync.

  $ rm $TESTTMP/wlockpre1
  $ hg cloud sync
  commitcloud: synchronizing 'testrepo' with 'user/test/default'
  visibility: read 1 heads: 1292cc1f1c17
  backing up stack rooted at 1292cc1f1c17
  pulling 79089e97b9e7 from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  visibility: removed 0 heads []; added 1 heads [79089e97b9e7]
  visibility: removed 0 heads []; added 1 heads [79089e97b9e7] (?)
  remote: pushing 1 commit:
  remote:     1292cc1f1c17  commit2
  commitcloud_sync: synced to workspace user/test/default version 2: 1 heads (0 omitted), 0 bookmarks (0 omitted), 0 remote bookmarks (0 omitted)
  visibility: wrote 2 heads: 79089e97b9e7, 1292cc1f1c17
  visibility: wrote 2 heads: 79089e97b9e7, 1292cc1f1c17 (?)
  commitcloud_sync: synced to workspace user/test/default version 3: 2 heads (0 omitted), 0 bookmarks (0 omitted), 0 remote bookmarks (0 omitted)
  commitcloud: commits synchronized
  finished in 0.00 sec

  $ tglogp
  visibility: read 2 heads: 79089e97b9e7, 1292cc1f1c17
  o  79089e97b9e7 draft 'commit1'
  │
  │ o  1292cc1f1c17 draft 'commit2'
  ├─╯
  @  df4f53cec30a public 'base'
  
Wait for the background backup to finish and check its output.

  $ hg debugwaitbackup
  $ cat $TESTTMP/bgsync.out
  commitcloud: synchronizing 'testrepo' with 'user/test/default'
  visibility: read 0 heads: 
  abort: commitcloud: failed to synchronize commits: 'repo changed while backing up'
  (please retry 'hg cloud sync')
  (please contact the Source Control Team if this error persists)
