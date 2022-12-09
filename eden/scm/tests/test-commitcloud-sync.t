#chg-compatible
#debugruntest-compatible
#inprocess-hg-incompatible
  $ setconfig workingcopy.ruststatus=False
  $ setconfig experimental.allowfilepeer=True

  $ configure dummyssh mutation-norecord
  $ enable amend directaccess commitcloud infinitepush rebase remotenames share smartlog

  $ setconfig infinitepush.branchpattern="re:scratch/.*"
  $ setconfig commitcloud.hostname=testhost
  $ setconfig visibility.verbose=true
  $ readconfig <<EOF
  > [alias]
  > trglog = log -G --template "{node|short} '{desc}' {bookmarks} {remotenames}\n"
  > descr = log -r '.' --template "{desc}"
  > EOF

  $ setconfig remotefilelog.reponame=server

  $ mkcommit() {
  >   echo "$1" > "$1"
  >   hg commit -Aqm "$1"
  > }

Full sync for repo1 and repo2 in quiet mode
This means cloud sync in the repo1, cloud sync in the repo2 and then again in the repo1
To be run if some test require full sync state before the test
  $ fullsync() {
  >   cd "$1"
  >   HGPLAIN=hint hg cloud sync -q
  >   cd ../"$2"
  >   HGPLAIN=hint hg cloud sync -q
  >   cd ../"$1"
  >   HGPLAIN=hint hg cloud sync -q
  >   cd ..
  > }

  $ newserver server
  $ mkcommit "base"
  $ hg bookmark publicbookmark1
  $ cd ..

Make shared part of config
  $ cat >> shared.rc << EOF
  > [commitcloud]
  > servicetype = local
  > servicelocation = $TESTTMP
  > auth_help = visit https://localhost/oauth to generate a registration token
  > education_page = https://someurl.com/wiki/CommitCloud
  > EOF

Make the first clone of the server
  $ hg clone ssh://user@dummy/server client1 -q
  $ cd client1
  $ cat ../shared.rc >> .hg/hgrc

Joining:
  $ hg cloud sync
  abort: commitcloud: workspace error: undefined workspace
  (your repo is not connected to any workspace)
  (use 'hg cloud join --help' for more details)
  [255]

Run cloud status before setting any workspace
  $ hg cloud status
  You are not connected to any workspace

  $ hg cloud join -w feature
  commitcloud: this repository is now connected to the 'user/test/feature' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/feature'
  commitcloud: commits synchronized
  finished in * (glob)

Run cloud status after setting a workspace
  $ hg cloud status
  Workspace: feature
  Raw Workspace Name: user/test/feature
  Automatic Sync (on local changes): OFF
  Automatic Sync via 'Scm Daemon' (on remote changes): OFF
  Last Sync Version: 1
  Last Sync Heads: 0 (0 omitted)
  Last Sync Bookmarks: 0 (0 omitted)
  Last Sync Remote Bookmarks: 0
  Last Sync Time: * (glob)
  Last Sync Status: Success

  $ hg cloud leave
  commitcloud: this repository is now disconnected from the 'user/test/feature' workspace
  $ hg cloud join
  commitcloud: this repository is now connected to the 'user/test/default' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: commits synchronized
  finished in * (glob)

  $ cd ..

Make the second clone of the server
  $ hg clone ssh://user@dummy/server client2 -q
  $ cd client2
  $ cat ../shared.rc >> .hg/hgrc
  $ hg cloud join
  commitcloud: this repository is now connected to the 'user/test/default' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: commits synchronized
  finished in * (glob)

  $ cd ..

Run cloud status after setting workspace
  $ cd client1
  $ hg cloud status
  Workspace: default
  Raw Workspace Name: user/test/default
  Automatic Sync (on local changes): OFF
  Automatic Sync via 'Scm Daemon' (on remote changes): OFF
  Last Sync Version: 1
  Last Sync Heads: 0 (0 omitted)
  Last Sync Bookmarks: 0 (0 omitted)
  Last Sync Remote Bookmarks: 0
  Last Sync Time: * (glob)
  Last Sync Status: Success

Enable autosync
  $ setconfig infinitepushbackup.autobackup=true

Run cloud status after enabling autosync
  $ hg cloud status
  Workspace: default
  Raw Workspace Name: user/test/default
  Automatic Sync (on local changes): ON
  Automatic Sync via 'Scm Daemon' (on remote changes): OFF
  Last Sync Version: 1
  Last Sync Heads: 0 (0 omitted)
  Last Sync Bookmarks: 0 (0 omitted)
  Last Sync Remote Bookmarks: 0
  Last Sync Time: * (glob)
  Last Sync Status: Success

Disable autosync
  $ setconfig infinitepushbackup.autobackup=false
Run cloud status after disabling autosync
  $ hg cloud status
  Workspace: default
  Raw Workspace Name: user/test/default
  Automatic Sync (on local changes): OFF
  Automatic Sync via 'Scm Daemon' (on remote changes): OFF
  Last Sync Version: 1
  Last Sync Heads: 0 (0 omitted)
  Last Sync Bookmarks: 0 (0 omitted)
  Last Sync Remote Bookmarks: 0
  Last Sync Time: * (glob)
  Last Sync Status: Success

  $ cd ..


Make a commit in the first client, and sync it
  $ cd client1
  $ mkcommit "commit1"
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  backing up stack rooted at fa5d62c46fd7
  commitcloud: commits synchronized
  finished in * (glob)
  remote: pushing 1 commit:
  remote:     fa5d62c46fd7  commit1

Sync requires visibility
  $ hg cloud sync --config visibility.enabled=false
  reverting to tracking visibility through obsmarkers
  commitcloud: synchronizing 'server' with 'user/test/default'
  abort: commit cloud sync requires new-style visibility
  [255]
  $ cd ..

Sync from the second client - the commit should appear
  $ cd client2
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  pulling fa5d62c46fd7 from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  commitcloud: commits synchronized
  finished in * (glob)

  $ hg up -q tip
  $ tglog
  @  fa5d62c46fd7 'commit1'
  │
  o  d20a80d4def3 'base'
  
Make a commit from the second client and sync it
  $ mkcommit "commit2"
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  backing up stack rooted at fa5d62c46fd7
  commitcloud: commits synchronized
  finished in * (glob)
  remote: pushing 2 commits:
  remote:     fa5d62c46fd7  commit1
  remote:     02f6fc2b7154  commit2
  $ cd ..

On the first client, make a bookmark, then sync - the bookmark and new commit should be synced
  $ cd client1
  $ hg bookmark -r 'desc(base)' bookmark1
  switching to explicit tracking of visible commits
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  pulling 02f6fc2b7154 from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  commitcloud: commits synchronized
  finished in * (glob)
  $ tglog
  o  02f6fc2b7154 'commit2'
  │
  @  fa5d62c46fd7 'commit1'
  │
  o  d20a80d4def3 'base' bookmark1
  
  $ cd ..

Sync the bookmark back to the second client
  $ cd client2
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: commits synchronized
  finished in * (glob)
  $ tglog
  @  02f6fc2b7154 'commit2'
  │
  o  fa5d62c46fd7 'commit1'
  │
  o  d20a80d4def3 'base' bookmark1
  
Move the bookmark on the second client, and then sync it
  $ hg bookmark -r 'desc(commit2)' -f bookmark1
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: commits synchronized
  finished in * (glob)

  $ cd ..

Move the bookmark also on the first client, it should be forked in the sync
  $ cd client1
  $ hg bookmark -r 'desc(commit1)' -f bookmark1
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  bookmark1 changed locally and remotely, local bookmark renamed to bookmark1-testhost
  commitcloud: commits synchronized
  finished in * (glob)
  $ tglog
  o  02f6fc2b7154 'commit2' bookmark1
  │
  @  fa5d62c46fd7 'commit1' bookmark1-testhost
  │
  o  d20a80d4def3 'base'
  
  $ cd ..

Amend a commit
Try to push selectively
  $ cd client1
  $ echo more >> commit1
  $ hg amend --rebase -m "`hg descr | head -n1` amended"
  rebasing 02f6fc2b7154 "commit2" (bookmark1)

  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  backing up stack rooted at a7bb357e7299
  commitcloud: commits synchronized
  finished in * (glob)
  remote: pushing 2 commits:
  remote:     a7bb357e7299  commit1 amended
  remote:     48610b1a7ec0  commit2

  $ tglog
  o  48610b1a7ec0 'commit2' bookmark1
  │
  @  a7bb357e7299 'commit1 amended' bookmark1-testhost
  │
  o  d20a80d4def3 'base'
  

  $ cd ..

Sync the amended commit to the other client
  $ cd client2
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  pulling 48610b1a7ec0 from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  commitcloud: commits synchronized
  finished in * (glob)
  commitcloud: current revision 02f6fc2b7154 has been moved remotely to 48610b1a7ec0
  hint[commitcloud-update-on-move]: if you would like to update to the moved version automatically add
  [commitcloud]
  updateonmove = true
  to your .hgrc config file
  hint[hint-ack]: use 'hg hint --ack commitcloud-update-on-move' to silence these hints
  $ hg up -q tip
  $ tglog
  @  48610b1a7ec0 'commit2' bookmark1
  │
  o  a7bb357e7299 'commit1 amended' bookmark1-testhost
  │
  o  d20a80d4def3 'base'
  
  $ cd ..

Note: before running this test repos should be synced
Test goal: test message that the revision has been moved
Description:
Amend a commit on client1 that is current for client2
Expected result: the message telling that revision has been moved to another revision
  $ cd client1
  $ hg up bookmark1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark bookmark1)
  $ hg amend -m "`hg descr | head -n1` amended"
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  backing up stack rooted at a7bb357e7299
  commitcloud: commits synchronized
  finished in * (glob)
  remote: pushing 2 commits:
  remote:     a7bb357e7299  commit1 amended
  remote:     41f3b9359864  commit2 amended

  $ cd ..

  $ cd client2
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  pulling 41f3b9359864 from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  commitcloud: commits synchronized
  finished in * (glob)
  commitcloud: current revision 48610b1a7ec0 has been moved remotely to 41f3b9359864
  hint[commitcloud-update-on-move]: if you would like to update to the moved version automatically add
  [commitcloud]
  updateonmove = true
  to your .hgrc config file
  hint[hint-ack]: use 'hg hint --ack commitcloud-update-on-move' to silence these hints
  $ tglog
  o  41f3b9359864 'commit2 amended' bookmark1
  │
  │ @  48610b1a7ec0 'commit2'
  ├─╯
  o  a7bb357e7299 'commit1 amended' bookmark1-testhost
  │
  o  d20a80d4def3 'base'
  

  $ cd ..
  $ fullsync client1 client2

Note: before running this test repos should be synced
Test goal: test move logic for commit with single amend
Description:
This test amends revision 41f3b9359864 at the client1
Client2 original position points to the same revision 41f3b9359864
Expected result: client2 should be moved to the amended version
  $ cd client1
  $ hg id -i
  41f3b9359864
  $ echo 1 > file.txt && hg addremove && hg amend -m "`hg descr | head -n1` amended"
  adding file.txt
  $ hg id -i
  8134e74ecdc8
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  backing up stack rooted at a7bb357e7299
  commitcloud: commits synchronized
  finished in * (glob)
  remote: pushing 2 commits:
  remote:     a7bb357e7299  commit1 amended
  remote:     8134e74ecdc8  commit2 amended amended

  $ cd ..

  $ cd client2
  $ cat >> .hg/hgrc << EOF
  > [commitcloud]
  > updateonmove=true
  > EOF
  $ hg up 41f3b9359864
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  pulling 8134e74ecdc8 from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  commitcloud: commits synchronized
  finished in * (glob)
  commitcloud: current revision 41f3b9359864 has been moved remotely to 8134e74ecdc8
  updating to 8134e74ecdc8
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ tglog
  @  8134e74ecdc8 'commit2 amended amended' bookmark1
  │
  o  a7bb357e7299 'commit1 amended' bookmark1-testhost
  │
  o  d20a80d4def3 'base'
  

  $ cd ..
  $ fullsync client1 client2

Note: before running this test repos should be synced
Test goal: test move logic for commit with multiple ammends
Description:
This test amends revision 41f3b9359864 2 times in the client1
The client2 original position points also to the revision 41f3b9359864
Expected result: move should not happen, expect a message that move is ambiguous
  $ cd client1
  $ hg up 41f3b9359864 -q
  $ echo 1 > filea.txt && hg addremove && hg amend -m "`hg descr | head -n1` amended"
  adding filea.txt
  $ hg id -i
  abd5311ab3c6
  $ hg up 41f3b9359864 -q
  $ echo 1 > fileb.txt && hg addremove && hg amend -m "`hg descr | head -n1` amended"
  adding fileb.txt
  $ hg id -i
  cebbb614447e
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  backing up stack rooted at a7bb357e7299
  commitcloud: commits synchronized
  finished in * (glob)
  remote: pushing 3 commits:
  remote:     a7bb357e7299  commit1 amended
  remote:     abd5311ab3c6  commit2 amended amended
  remote:     cebbb614447e  commit2 amended amended

  $ cd ..

  $ cd client2
  $ hg up 41f3b9359864 -q
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  pulling abd5311ab3c6 cebbb614447e from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  adding changesets
  adding manifests
  adding file changes
  commitcloud: commits synchronized
  finished in * (glob)
  commitcloud: current revision 41f3b9359864 has been replaced remotely with multiple revisions
  (run 'hg goto HASH' to go to the desired revision)
  $ tglog
  o  cebbb614447e 'commit2 amended amended'
  │
  │ o  abd5311ab3c6 'commit2 amended amended'
  ├─╯
  │ o  8134e74ecdc8 'commit2 amended amended' bookmark1
  ├─╯
  │ @  41f3b9359864 'commit2 amended'
  ├─╯
  o  a7bb357e7299 'commit1 amended' bookmark1-testhost
  │
  o  d20a80d4def3 'base'
  
  $ cd ..
  $ fullsync client1 client2

Note: before running this test repos should be synced
Test goal: test move logic for several amends in row
Description:
Make 3 amends on client1 for the revision abd5311ab3c6
On client2 the original position points to the same revision abd5311ab3c6
Expected result: client2 should be moved to fada67350ab0
  $ cd client1
  $ hg up abd5311ab3c6 -q
  $ echo 2 >> filea.txt && hg amend -m "`hg descr | head -n1` amended"
  $ hg id -i
  f4ea578a3184
  $ echo 3 >> filea.txt && hg amend -m "`hg descr | head -n1` amended"
  $ hg id -i
  acf8d3fd70ac
  $ echo 4 >> filea.txt && hg amend -m "`hg descr | head -n1` amended"
  $ hg id -i
  fada67350ab0
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  backing up stack rooted at a7bb357e7299
  commitcloud: commits synchronized
  finished in * (glob)
  remote: pushing 2 commits:
  remote:     a7bb357e7299  commit1 amended
  remote:     fada67350ab0  commit2 amended amended amended amended amended

  $ cd ..

  $ cd client2
  $ hg up abd5311ab3c6 -q
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  pulling fada67350ab0 from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  commitcloud: commits synchronized
  finished in * (glob)
  commitcloud: current revision abd5311ab3c6 has been moved remotely to fada67350ab0
  updating to fada67350ab0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cd ..
  $ fullsync client1 client2

Note: before running this test repos should be synced
Test goal: test move logic for several rebases and amends
Description:
Client1 handles several operations on the rev cebbb614447e: rebase, amend, rebase, amend
Client2 original position is cebbb614447e
Expected result: client2 should be moved to 68e035cc1996
  $ cd client1
  $ hg up cebbb614447e -q
  $ tglog
  o  fada67350ab0 'commit2 amended amended amended amended amended'
  │
  │ @  cebbb614447e 'commit2 amended amended'
  ├─╯
  │ o  8134e74ecdc8 'commit2 amended amended' bookmark1
  ├─╯
  o  a7bb357e7299 'commit1 amended' bookmark1-testhost
  │
  o  d20a80d4def3 'base'
  
  $ hg rebase -s cebbb614447e -d d20a80d4def3 -m "`hg descr | head -n1` rebased" --collapse
  rebasing cebbb614447e "commit2 amended amended"
  $ echo 5 >> filea.txt && hg amend -m "`hg descr | head -n1` amended"
  $ hg id -i
  99e818be5af0
  $ hg rebase -s 99e818be5af0 -d a7bb357e7299 -m "`hg descr | head -n1` rebased" --collapse
  rebasing 99e818be5af0 "commit2 amended amended rebased amended"
  $ echo 6 >> filea.txt && hg amend -m "`hg descr | head -n1` amended"
  $ tglog -r '.'
  @  68e035cc1996 'commit2 amended amended rebased amended rebased amended'
  │
  ~
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  backing up stack rooted at a7bb357e7299
  commitcloud: commits synchronized
  finished in * (glob)
  remote: pushing 2 commits:
  remote:     a7bb357e7299  commit1 amended
  remote:     68e035cc1996  commit2 amended amended rebased amended rebased am

  $ cd ..

  $ cd client2
  $ hg up cebbb614447e -q
  $ tglog
  o  fada67350ab0 'commit2 amended amended amended amended amended'
  │
  │ @  cebbb614447e 'commit2 amended amended'
  ├─╯
  │ o  8134e74ecdc8 'commit2 amended amended' bookmark1
  ├─╯
  o  a7bb357e7299 'commit1 amended' bookmark1-testhost
  │
  o  d20a80d4def3 'base'
  
  $ hg cloud sync -q
  $ tglog -r '.'
  @  68e035cc1996 'commit2 amended amended rebased amended rebased amended'
  │
  ~

Clean up by hiding some commits, and create a new stack

  $ hg goto d20a80d4def3 -q
  $ hg hide a7bb357e7299
  hiding commit a7bb357e7299 "commit1 amended"
  hiding commit 8134e74ecdc8 "commit2 amended amended"
  hiding commit fada67350ab0 "commit2 amended amended amended amended amended"
  hiding commit 68e035cc1996 "commit2 amended amended rebased amended rebased am"
  4 changesets hidden
  removing bookmark 'bookmark1' (was at: 8134e74ecdc8)
  removing bookmark 'bookmark1-testhost' (was at: a7bb357e7299)
  2 bookmarks removed
  $ hg bookmark testbookmark
  $ hg cloud sync -q
  $ mkcommit "stack commit 1"
  $ mkcommit "stack commit 2"
  $ hg cloud sync -q
  $ cd ..
  $ cd client1
  $ hg goto d20a80d4def3 -q
  $ hg cloud sync -q
  $ tglog
  o  f2ccc2716735 'stack commit 2' testbookmark
  │
  o  74473a0f136f 'stack commit 1'
  │
  @  d20a80d4def3 'base'
  
Test race between cloud sync and another transaction

  $ hg next -q
  [74473a] stack commit 1

  $ hg amend -m "race attempt" --no-rebase
  hint[amend-restack]: descendants of 74473a0f136f are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints

Create a simultanous rebase and cloud sync, where the cloud sync has won the
race for the lock

  $ touch $TESTTMP/lockdelay
  $ HGPREWLOCKFILE=$TESTTMP/lockdelay hg rebase --restack --config extensions.lockdelay=$TESTDIR/lockdelay.py >$TESTTMP/racerebase.out 2>&1 &
  $ HGPOSTLOCKFILE=$TESTTMP/lockdelay hg cloud sync -q --config extensions.lockdelay=$TESTDIR/lockdelay.py >$TESTTMP/racecloudsync.out 2>&1 &
  $ sleep 1

Let them both run together
  $ rm $TESTTMP/lockdelay

Wait for them to complete and then do another cloud sync
  $ wait
  $ hg cloud sync -q
  $ grep rebasing $TESTTMP/racerebase.out
  rebasing f2ccc2716735 "stack commit 2" (testbookmark)
  $ tglog
  o  715c1454ae33 'stack commit 2' testbookmark
  │
  @  4b4f26511f8b 'race attempt'
  │
  o  d20a80d4def3 'base'
  
  $ cd ..
  $ cd client2
  $ hg cloud sync -q
  $ tglog
  @  715c1454ae33 'stack commit 2' testbookmark
  │
  o  4b4f26511f8b 'race attempt'
  │
  o  d20a80d4def3 'base'
  
  $ cd ..

Test interactions with  share extension

Create a shared client directory

  $ hg share client1 client1b
  updating working directory
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat shared.rc >> client1b/.hg/hgrc
  $ cd client1b
  $ tglog
  @  715c1454ae33 'stack commit 2' testbookmark
  │
  o  4b4f26511f8b 'race attempt'
  │
  o  d20a80d4def3 'base'
  
Make a new commit to be shared

  $ mkcommit "shared commit"
  $ tglog
  @  2c0ce859e76a 'shared commit'
  │
  o  715c1454ae33 'stack commit 2' testbookmark
  │
  o  4b4f26511f8b 'race attempt'
  │
  o  d20a80d4def3 'base'
  
Check cloud sync backs up the commit

  $ hg cloud check
  2c0ce859e76ae60f6f832279c75fae4d61da6be2 not backed up
  $ hg cloud sync -q
  $ hg cloud check
  2c0ce859e76ae60f6f832279c75fae4d61da6be2 backed up

Check cloud sync in the source repo doesn't need to do anything

  $ cd ../client1
  $ tglog
  o  2c0ce859e76a 'shared commit'
  │
  o  715c1454ae33 'stack commit 2' testbookmark
  │
  @  4b4f26511f8b 'race attempt'
  │
  o  d20a80d4def3 'base'
  
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: commits synchronized
  finished in * (glob)

Check cloud sync pulls in the shared commit in the other client

  $ cd ../client2
  $ hg cloud sync -q
  $ tglog
  o  2c0ce859e76a 'shared commit'
  │
  @  715c1454ae33 'stack commit 2' testbookmark
  │
  o  4b4f26511f8b 'race attempt'
  │
  o  d20a80d4def3 'base'
  
 

Check handling of scm daemon options:

Check '--workspace-version' option
  $ hg cloud sync --workspace-version 1
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: this version has been already synchronized

Check '--check-autosync-enabled' option
  $ hg cloud sync --check-autosync-enabled
  commitcloud: background operations are currently disabled
  $ hg cloud sync --check-autosync-enabled --config infinitepushbackup.autobackup=true
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: commits synchronized
  finished in * (glob)

Check '--raw-workspace-name' option
  $ hg cloud sync --raw-workspace-name 'user/test/default'
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: commits synchronized
  finished in 0.00 sec
  $ hg cloud sync --raw-workspace-name 'user/test/other'
  current workspace is different than the workspace to sync
  [1]

Check handling of failures
Simulate failure to backup a commit by setting the server maxbundlesize limit very low

  $ cp ../server/.hg/hgrc $TESTTMP/server-hgrc.bak
  $ cat >> ../server/.hg/hgrc << EOF
  > [infinitepush]
  > maxbundlesize = 0
  > EOF
  $ hg up testbookmark
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark testbookmark)
  $ hg metaedit -r 2c0ce859e76a -m 'shared commit updated'
  $ mkcommit toobig
  $ hg book toobig
  $ tglog
  @  9bd68ef10d6b 'toobig' testbookmark toobig
  │
  │ o  a6b97eebbf74 'shared commit updated'
  ├─╯
  o  715c1454ae33 'stack commit 2'
  │
  o  4b4f26511f8b 'race attempt'
  │
  o  d20a80d4def3 'base'
  
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  backing up stack rooted at 4b4f26511f8b
  remote: pushing 4 commits:
  remote:     4b4f26511f8b  race attempt
  remote:     715c1454ae33  stack commit 2
  remote:     a6b97eebbf74  shared commit updated
  remote:     9bd68ef10d6b  toobig
  push failed: bundle is too big: 3104 bytes. max allowed size is 0 MB
  retrying push with discovery
  searching for changes
  remote: pushing 4 commits:
  remote:     4b4f26511f8b  race attempt
  remote:     715c1454ae33  stack commit 2
  remote:     a6b97eebbf74  shared commit updated
  remote:     9bd68ef10d6b  toobig
  push of stack 4b4f26511f8b failed: bundle is too big: 3104 bytes. max allowed size is 0 MB
  retrying each head individually
  remote: pushing 3 commits:
  remote:     4b4f26511f8b  race attempt
  remote:     715c1454ae33  stack commit 2
  remote:     a6b97eebbf74  shared commit updated
  push failed: bundle is too big: 2441 bytes. max allowed size is 0 MB
  retrying push with discovery
  searching for changes
  remote: pushing 3 commits:
  remote:     4b4f26511f8b  race attempt
  remote:     715c1454ae33  stack commit 2
  remote:     a6b97eebbf74  shared commit updated
  push of head a6b97eebbf74 failed: bundle is too big: 2441 bytes. max allowed size is 0 MB
  remote: pushing 3 commits:
  remote:     4b4f26511f8b  race attempt
  remote:     715c1454ae33  stack commit 2
  remote:     9bd68ef10d6b  toobig
  push failed: bundle is too big: 2331 bytes. max allowed size is 0 MB
  retrying push with discovery
  searching for changes
  remote: pushing 3 commits:
  remote:     4b4f26511f8b  race attempt
  remote:     715c1454ae33  stack commit 2
  remote:     9bd68ef10d6b  toobig
  push of head 9bd68ef10d6b failed: bundle is too big: 2331 bytes. max allowed size is 0 MB
  commitcloud: failed to synchronize * (glob)
  commitcloud: failed to synchronize * (glob)
  finished in * (glob)

Run cloud status after failing to synchronize
  $ hg cloud status
  Workspace: default
  Raw Workspace Name: user/test/default
  Automatic Sync (on local changes): OFF
  Automatic Sync via 'Scm Daemon' (on remote changes): OFF
  Last Sync Version: 18
  Last Sync Heads: 1 (0 omitted)
  Last Sync Bookmarks: 1 (0 omitted)
  Last Sync Remote Bookmarks: 0
  Last Sync Time: * (glob)
  Last Sync Status: Failed

  $ hg cloud check -r .
  9bd68ef10d6bdb8ebf3273a7b91bc4f3debe2a87 not backed up

Set the limit back high.  Sync in the other repo and check it still looks ok
(but with the failed commits missing).

  $ mv $TESTTMP/server-hgrc.bak ../server/.hg/hgrc
  $ cd ../client1
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: commits synchronized
  finished in * (glob)
  $ tglog
  o  715c1454ae33 'stack commit 2' testbookmark
  │
  @  4b4f26511f8b 'race attempt'
  │
  o  d20a80d4def3 'base'
  

Now sync in the repo we failed in.  This time it should work.

  $ cd ../client2
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  backing up stack rooted at 4b4f26511f8b
  commitcloud: commits synchronized
  finished in * (glob)
  remote: pushing 4 commits:
  remote:     4b4f26511f8b  race attempt
  remote:     715c1454ae33  stack commit 2
  remote:     a6b97eebbf74  shared commit updated
  remote:     9bd68ef10d6b  toobig
  $ hg cloud check -r .
  9bd68ef10d6bdb8ebf3273a7b91bc4f3debe2a87 backed up
  $ tglog
  @  9bd68ef10d6b 'toobig' testbookmark toobig
  │
  │ o  a6b97eebbf74 'shared commit updated'
  ├─╯
  o  715c1454ae33 'stack commit 2'
  │
  o  4b4f26511f8b 'race attempt'
  │
  o  d20a80d4def3 'base'
  

And the commits should now be availble in the other client.

  $ cd ../client1
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  pulling a6b97eebbf74 9bd68ef10d6b from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  adding changesets
  adding manifests
  adding file changes
  commitcloud: commits synchronized
  finished in * (glob)
  $ tglog
  o  a6b97eebbf74 'shared commit updated'
  │
  │ o  9bd68ef10d6b 'toobig' testbookmark toobig
  ├─╯
  o  715c1454ae33 'stack commit 2'
  │
  @  4b4f26511f8b 'race attempt'
  │
  o  d20a80d4def3 'base'
  
Clean up

  $ hg up -q -r d20a80d4def38df63a4b330b7fb688f3d4cae1e3
  $ hg hide -q 4b4f26511f8b
  $ cd ..
  $ fullsync client1 client2
  $ cd client2
  $ hg up -q -r d20a80d4def38df63a4b330b7fb688f3d4cae1e3
  $ cd ../client1

Make two stacks

  $ mkcommit 'stack 1 first'
  $ mkcommit 'stack 1 second'
  $ hg up -q -r d20a80d4def38df63a4b330b7fb688f3d4cae1e3
  $ mkcommit 'stack 2 first'
  $ mkcommit 'stack 2 second'
  $ tglog
  @  799d22972c4e 'stack 2 second'
  │
  o  3597ff85ead0 'stack 2 first'
  │
  │ o  9a3e7907fd5c 'stack 1 second'
  │ │
  │ o  e58a6603d256 'stack 1 first'
  ├─╯
  o  d20a80d4def3 'base'
  
Make one of the commits public when it shouldn't be.

  $ hg debugmakepublic e58a6603d256
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  backing up stack rooted at 9a3e7907fd5c
  remote: abort: 00changelog.i@e58a6603d256: unknown parent!
  push failed: stream ended unexpectedly (got 0 bytes, expected 4)
  retrying push with discovery
  searching for changes
  backing up stack rooted at 3597ff85ead0
  commitcloud: commits synchronized
  finished in * (glob)
  remote: pushing 2 commits:
  remote:     e58a6603d256  stack 1 first
  remote:     9a3e7907fd5c  stack 1 second
  remote: pushing 2 commits:
  remote:     3597ff85ead0  stack 2 first
  remote:     799d22972c4e  stack 2 second

Commit still becomes available in the other repo

  $ cd ../client2
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  pulling 9a3e7907fd5c 799d22972c4e from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  adding changesets
  adding manifests
  adding file changes
  commitcloud: commits synchronized
  finished in * (glob)
  $ tglog
  o  9a3e7907fd5c 'stack 1 second'
  │
  o  e58a6603d256 'stack 1 first'
  │
  │ o  799d22972c4e 'stack 2 second'
  │ │
  │ o  3597ff85ead0 'stack 2 first'
  ├─╯
  @  d20a80d4def3 'base'
  
Fix up that public commit, set it back to draft
  $ cd ../client1
  $ hg debugmakepublic -d -r e58a6603d256

Make a public commit and put two bookmarks on it
  $ cd ../server
  $ mkcommit 'public'
  $ hg bookmark publicbookmark2

Pull it into one of the clients and rebase one of the stacks onto it
  $ cd ../client1
  $ hg pull -q
  $ hg trglog
  o  acd5b9e8c656 'public'  default/publicbookmark1 default/publicbookmark2
  │
  │ @  799d22972c4e 'stack 2 second'
  │ │
  │ o  3597ff85ead0 'stack 2 first'
  ├─╯
  │ o  9a3e7907fd5c 'stack 1 second'
  │ │
  │ o  e58a6603d256 'stack 1 first'
  ├─╯
  o  d20a80d4def3 'base'
  
  $ hg rebase -s e58a6603d256 -d publicbookmark1
  rebasing e58a6603d256 "stack 1 first"
  rebasing 9a3e7907fd5c "stack 1 second"
  $ hg cloud sync -q

Create another public commit on the server, moving one of the bookmarks
  $ cd ../server
  $ mkcommit 'public 2'
  $ tglog
  @  97250524560a 'public 2' publicbookmark2
  │
  o  acd5b9e8c656 'public' publicbookmark1
  │
  o  d20a80d4def3 'base'
  
Sync this onto the second client, the remote bookmarks don't change.
  $ cd ../client2
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  pulling 2da6c73964b8 from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  adding changesets
  adding manifests
  adding file changes
  commitcloud: commits synchronized
  finished in * (glob)
  $ hg trglog
  o  2da6c73964b8 'stack 1 second'
  │
  o  5df7c1d8d8ab 'stack 1 first'
  │
  o  acd5b9e8c656 'public'
  │
  │ o  799d22972c4e 'stack 2 second'
  │ │
  │ o  3597ff85ead0 'stack 2 first'
  ├─╯
  @  d20a80d4def3 'base'  default/publicbookmark1
  
Do a pull on this client.  The remote bookmarks now get updated.
  $ hg pull
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  $ hg trglog
  o  97250524560a 'public 2'  default/publicbookmark2
  │
  │ o  2da6c73964b8 'stack 1 second'
  │ │
  │ o  5df7c1d8d8ab 'stack 1 first'
  ├─╯
  o  acd5b9e8c656 'public'  default/publicbookmark1
  │
  │ o  799d22972c4e 'stack 2 second'
  │ │
  │ o  3597ff85ead0 'stack 2 first'
  ├─╯
  @  d20a80d4def3 'base'
  
Rebase the commits again, and resync to the first client.
  $ hg rebase -s 5df7c1d8d8ab -d publicbookmark2
  rebasing 5df7c1d8d8ab "stack 1 first"
  rebasing 2da6c73964b8 "stack 1 second"
  $ hg cloud sync -q
  $ cd ../client1
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  pulling af621240884f from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  adding changesets
  adding manifests
  adding file changes
  commitcloud: commits synchronized
  finished in * (glob)
  $ hg trglog
  o  af621240884f 'stack 1 second'
  │
  o  81cd67693e59 'stack 1 first'
  │
  o  97250524560a 'public 2'
  │
  o  acd5b9e8c656 'public'  default/publicbookmark1 default/publicbookmark2
  │
  │ @  799d22972c4e 'stack 2 second'
  │ │
  │ o  3597ff85ead0 'stack 2 first'
  ├─╯
  o  d20a80d4def3 'base'
  
A final pull gets everything in sync here, too.
  $ hg pull -q
  $ hg trglog
  o  af621240884f 'stack 1 second'
  │
  o  81cd67693e59 'stack 1 first'
  │
  o  97250524560a 'public 2'  default/publicbookmark2
  │
  o  acd5b9e8c656 'public'  default/publicbookmark1
  │
  │ @  799d22972c4e 'stack 2 second'
  │ │
  │ o  3597ff85ead0 'stack 2 first'
  ├─╯
  o  d20a80d4def3 'base'
  
Check subscription when join/leave and also scm service health check
  $ cat >> .hg/hgrc << EOF
  > [commitcloud]
  > subscription_enabled = true
  > subscriber_service_tcp_port = 15432
  > connected_subscribers_path = $TESTTMP
  > EOF
  $ hg cloud sync -q
  $ cat $TESTTMP/.commitcloud/joined/*
  [commitcloud]
  workspace=user/test/default
  repo_name=server
  repo_root=$TESTTMP/client1/.hg
  $ hg cloud leave
  commitcloud: this repository is now disconnected from the 'user/test/default' workspace
  $ ls $TESTTMP/.commitcloud/joined/
  $ hg cloud join -q
  $ cat $TESTTMP/.commitcloud/joined/*
  [commitcloud]
  workspace=user/test/default
  repo_name=server
  repo_root=$TESTTMP/client1/.hg

  $ cd ..

Rejoin
  $ rm -rf client2
  $ mkdir client2

  $ mkdir $TESTTMP/otherservicelocation
  $ mkdir $TESTTMP/othertokenlocation

  $ hg clone ssh://user@dummy/server client2 -q
  $ cd client2
  $ cat ../shared.rc >> .hg/hgrc

Reconnect to a service where the workspace is brand new.  This should work.

  $ hg cloud reconnect --config "commitcloud.servicelocation=$TESTTMP/otherservicelocation"
  commitcloud: attempting to connect to the 'user/test/default' workspace for the 'server' repo
  commitcloud: this repository is now connected to the 'user/test/default' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: commits synchronized
  finished in * (glob)

  $ hg cloud disconnect
  commitcloud: this repository is now disconnected from the 'user/test/default' workspace

Reconnect to the default repository.  This should work and pull in the commits.
  $ hg cloud reconnect
  commitcloud: attempting to connect to the 'user/test/default' workspace for the 'server' repo
  commitcloud: this repository is now connected to the 'user/test/default' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/default'
  pulling 799d22972c4e af621240884f from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  adding changesets
  adding manifests
  adding file changes
  commitcloud: commits synchronized
  finished in * (glob)
  hint[commitcloud-switch]: the following commitcloud workspaces (backups) are available for this repo:
  user/test/feature
  user/test/default
  run `hg cloud list` inside the repo to see all your workspaces,
  find the one the repo is connected to and learn how to switch between them
  hint[hint-ack]: use 'hg hint --ack commitcloud-switch' to silence these hints

  $ hg trglog
  o  af621240884f 'stack 1 second'
  │
  o  81cd67693e59 'stack 1 first'
  │
  │ o  799d22972c4e 'stack 2 second'
  │ │
  │ o  3597ff85ead0 'stack 2 first'
  │ │
  @ │  97250524560a 'public 2'  default/publicbookmark2
  │ │
  o │  acd5b9e8c656 'public'  default/publicbookmark1
  ├─╯
  o  d20a80d4def3 'base'
  

Reconnecting while already connected does nothing.
  $ hg cloud reconnect

  $ hg cloud disconnect
  commitcloud: this repository is now disconnected from the 'user/test/default' workspace

Completely remove commit cloud config and then pull with automigrate enabled.
This should also reconnect.

  $ rm .hg/store/commitcloudrc
  $ hg pull --config commitcloud.automigrate=true
  pulling from ssh://user@dummy/server
  searching for changes
  no changes found
Enable commit cloud background operations.
  $ setconfig infinitepushbackup.autobackup=true
  $ hg pull --config commitcloud.automigrate=true
  pulling from ssh://user@dummy/server
  searching for changes
  no changes found
  commitcloud: attempting to connect to the 'user/test/default' workspace for the 'server' repo
  commitcloud: this repository is now connected to the 'user/test/default' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: commits synchronized
  finished in * (glob)
  hint[commitcloud-switch]: the following commitcloud workspaces (backups) are available for this repo:
  user/test/feature
  user/test/default
  run `hg cloud list` inside the repo to see all your workspaces,
  find the one the repo is connected to and learn how to switch between them
  hint[hint-ack]: use 'hg hint --ack commitcloud-switch' to silence these hints

But not if already connected.
  $ hg pull --config commitcloud.automigrate=true
  pulling from ssh://user@dummy/server
  searching for changes
  no changes found

  $ hg cloud disconnect
  commitcloud: this repository is now disconnected from the 'user/test/default' workspace

And not if explicitly disconnected.
  $ hg pull --config commitcloud.automigrate=true
  pulling from ssh://user@dummy/server
  searching for changes
  no changes found

Pull with automigrate enabled and host-specific workspaces

  $ rm .hg/store/commitcloudrc
  $ hg pull --config commitcloud.automigrate=true --config commitcloud.automigratehostworkspace=true
  pulling from ssh://user@dummy/server
  searching for changes
  no changes found
  commitcloud: attempting to connect to the 'user/test/testhost' workspace for the 'server' repo
  commitcloud: this repository is now connected to the 'user/test/testhost' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/testhost'
  commitcloud: commits synchronized
  finished in * (glob)

Host-specific workspace now exists, so it should be chosen as the one to connect to
  $ hg cloud leave
  commitcloud: this repository is now disconnected from the 'user/test/testhost' workspace
  $ hg cloud reconnect
  commitcloud: attempting to connect to the 'user/test/testhost' workspace for the 'server' repo
  commitcloud: this repository is now connected to the 'user/test/testhost' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/testhost'
  commitcloud: commits synchronized
  finished in * (glob)
  hint[commitcloud-switch]: the following commitcloud workspaces (backups) are available for this repo:
  user/test/feature
  user/test/default
  user/test/testhost
  run `hg cloud list` inside the repo to see all your workspaces,
  find the one the repo is connected to and learn how to switch between them
  hint[hint-ack]: use 'hg hint --ack commitcloud-switch' to silence these hints

  $ hg cloud join --switch -w default
  commitcloud: synchronizing 'server' with 'user/test/testhost'
  commitcloud: commits synchronized
  finished in * (glob)
  commitcloud: now this repository will be switched from the 'user/test/testhost' to the 'user/test/default' workspace
  commitcloud: this repository is now connected to the 'user/test/default' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: commits synchronized
  finished in * (glob)
