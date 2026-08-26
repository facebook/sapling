#require no-eden

  $ setconfig devel.collapse-traceback=true

  $ configure dummyssh mutation-norecord
  $ enable amend directaccess commitcloud rebase share smartlog

  $ setconfig infinitepush.branchpattern="re:scratch/.*"
  $ setconfig commitcloud.hostname=testhost
  $ setconfig commitcloud.upload_retry_attempts=1
  $ setconfig visibility.verbose=true
  $ setconfig remotenames.selectivepulldefault=publicbookmark1,publicbookmark2
  $ readconfig <<EOF
  > [alias]
  > trglog = log -G --template "{node|short} '{desc}' {bookmarks} {remotenames}\n"
  > descr = log -r '.' --template "{desc}"
  > EOF

  $ mkcommit() {
  >   echo "$1" > "$1"
  >   sl commit -Aqm "$1"
  > }

Full sync for repo1 and repo2 in quiet mode
This means cloud sync in the repo1, cloud sync in the repo2 and then again in the repo1
To be run if some test require full sync state before the test
  $ fullsync() {
  >   cd "$1"
  >   HGPLAIN=hint sl cloud sync -q
  >   cd ../"$2"
  >   HGPLAIN=hint sl cloud sync -q
  >   cd ../"$1"
  >   HGPLAIN=hint sl cloud sync -q
  >   cd ..
  > }

  $ newserver server
  $ mkcommit "base"
  $ sl bookmark publicbookmark1
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
  $ sl clone ssh://user@dummy/server client1 -q
  $ cd client1
  $ cat ../shared.rc >> .sl/config

Joining:
  $ sl cloud sync
  abort: commitcloud: workspace error: undefined workspace
  (your repo is not connected to any workspace)
  (use 'sl cloud join --help' for more details)
  [255]

Run cloud status before setting any workspace
  $ sl cloud status
  You are not connected to any workspace

  $ sl cloud join -w feature
  commitcloud: this repository is now connected to the 'user/test/feature' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/feature'
  commitcloud: nothing to upload
  commitcloud: commits synchronized
  finished in * (glob)

Run cloud status after setting a workspace
  $ sl cloud status
  Workspace: feature
  Raw Workspace Name: user/test/feature
  Automatic Sync (on local changes): OFF
  Automatic Sync via 'Scm Daemon' (on remote changes): OFF
  Last Sync Version: 1
  Last Sync Heads: 0 (0 omitted)
  Last Sync Bookmarks: 0 (0 omitted)
  Last Sync Remote Bookmarks: 1
  Last Sync Time: * (glob)
  Last Sync Status: Success

  $ sl cloud leave
  commitcloud: this repository is now disconnected from the 'user/test/feature' workspace
  $ sl cloud join
  commitcloud: this repository is now connected to the 'user/test/default' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: nothing to upload
  commitcloud: commits synchronized
  finished in * (glob)

  $ cd ..

Make the second clone of the server
  $ sl clone ssh://user@dummy/server client2 -q
  $ cd client2
  $ cat ../shared.rc >> .sl/config
  $ sl cloud join
  commitcloud: this repository is now connected to the 'user/test/default' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: nothing to upload
  commitcloud: commits synchronized
  finished in * (glob)

  $ cd ..

Run cloud status after setting workspace
  $ cd client1
  $ sl cloud status
  Workspace: default
  Raw Workspace Name: user/test/default
  Automatic Sync (on local changes): OFF
  Automatic Sync via 'Scm Daemon' (on remote changes): OFF
  Last Sync Version: 1
  Last Sync Heads: 0 (0 omitted)
  Last Sync Bookmarks: 0 (0 omitted)
  Last Sync Remote Bookmarks: 1
  Last Sync Time: * (glob)
  Last Sync Status: Success

Enable autosync
  $ setconfig infinitepushbackup.autobackup=true

Run cloud status after enabling autosync
  $ sl cloud status
  Workspace: default
  Raw Workspace Name: user/test/default
  Automatic Sync (on local changes): ON
  Automatic Sync via 'Scm Daemon' (on remote changes): OFF
  Last Sync Version: 1
  Last Sync Heads: 0 (0 omitted)
  Last Sync Bookmarks: 0 (0 omitted)
  Last Sync Remote Bookmarks: 1
  Last Sync Time: * (glob)
  Last Sync Status: Success

Disable autosync
  $ setconfig infinitepushbackup.autobackup=false
Run cloud status after disabling autosync
  $ sl cloud status
  Workspace: default
  Raw Workspace Name: user/test/default
  Automatic Sync (on local changes): OFF
  Automatic Sync via 'Scm Daemon' (on remote changes): OFF
  Last Sync Version: 1
  Last Sync Heads: 0 (0 omitted)
  Last Sync Bookmarks: 0 (0 omitted)
  Last Sync Remote Bookmarks: 1
  Last Sync Time: * (glob)
  Last Sync Status: Success

  $ cd ..


Make a commit in the first client, and sync it
  $ cd client1
  $ mkcommit "commit1"
  $ sl cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: head 'fa5d62c46fd7' hasn't been uploaded yet
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  commitcloud: commits synchronized
  finished in * (glob)

Sync does not allow positional arguments
  $ sl cloud sync all
  sl cloud sync: invalid arguments
  (use 'sl cloud sync -h' to get help)
  [255]

Sync requires visibility
  $ sl cloud sync --config visibility.enabled=false
  reverting to tracking visibility through obsmarkers
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: nothing to upload
  abort: commit cloud sync requires new-style visibility
  [255]
  $ cd ..

Sync from the second client - the commit should appear
  $ cd client2
  $ sl cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: nothing to upload
  pulling fa5d62c46fd7 from ssh://user@dummy/server
  searching for changes
  commitcloud: commits synchronized
  finished in * (glob)

  $ sl up -q tip
  $ tglog
  @  fa5d62c46fd7 'commit1'
  │
  o  d20a80d4def3 'base'
  
Make a commit from the second client and sync it
  $ mkcommit "commit2"
  $ sl cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: head '02f6fc2b7154' hasn't been uploaded yet
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  commitcloud: commits synchronized
  finished in * (glob)
  $ cd ..

On the first client, make a bookmark, then sync - the bookmark and new commit should be synced
  $ cd client1
  $ sl bookmark -r 'desc(base)' bookmark1
  switching to explicit tracking of visible commits
  $ sl cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: nothing to upload
  pulling 02f6fc2b7154 from ssh://user@dummy/server
  searching for changes
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
  $ sl cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: nothing to upload
  commitcloud: commits synchronized
  finished in * (glob)
  $ tglog
  @  02f6fc2b7154 'commit2'
  │
  o  fa5d62c46fd7 'commit1'
  │
  o  d20a80d4def3 'base' bookmark1
  
Move the bookmark on the second client, and then sync it
  $ sl bookmark -r 'desc(commit2)' -f bookmark1
  $ sl cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: nothing to upload
  commitcloud: commits synchronized
  finished in * (glob)

  $ cd ..

Move the bookmark also on the first client, it should be forked in the sync
  $ cd client1
  $ sl bookmark -r 'desc(commit1)' -f bookmark1
  $ sl cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: nothing to upload
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
  $ sl amend --rebase -m "`sl descr | head -n1` amended"
  rebasing 02f6fc2b7154 "commit2" (bookmark1)

  $ sl cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: head '48610b1a7ec0' hasn't been uploaded yet
  edenapi: queue 2 commits for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 2 trees for upload
  edenapi: uploaded 2 trees
  edenapi: uploaded 2 changesets
  commitcloud: commits synchronized
  finished in * (glob)

  $ tglog
  o  48610b1a7ec0 'commit2' bookmark1
  │
  @  a7bb357e7299 'commit1 amended' bookmark1-testhost
  │
  o  d20a80d4def3 'base'
  

  $ cd ..

Sync the amended commit to the other client
  $ cd client2
  $ sl cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: nothing to upload
  pulling 48610b1a7ec0 from ssh://user@dummy/server
  searching for changes
  commitcloud: commits synchronized
  finished in * (glob)
  commitcloud: current revision 02f6fc2b7154 has been moved remotely to 48610b1a7ec0
  $ sl up -q tip
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
  $ sl up bookmark1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark bookmark1)
  $ sl amend -m "`sl descr | head -n1` amended"
  $ sl cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: head '41f3b9359864' hasn't been uploaded yet
  edenapi: queue 1 commit for upload
  edenapi: queue 0 files for upload
  edenapi: queue 0 trees for upload
  edenapi: uploaded 1 changeset
  commitcloud: commits synchronized
  finished in * (glob)

  $ cd ..

  $ cd client2
  $ sl cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: nothing to upload
  pulling 41f3b9359864 from ssh://user@dummy/server
  searching for changes
  commitcloud: commits synchronized
  finished in * (glob)
  commitcloud: current revision 48610b1a7ec0 has been moved remotely to 41f3b9359864
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
  $ sl id -i
  41f3b9359864
  $ echo 1 > file.txt && sl addremove && sl amend -m "`sl descr | head -n1` amended"
  adding file.txt
  $ sl id -i
  8134e74ecdc8
  $ sl cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: head '8134e74ecdc8' hasn't been uploaded yet
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  commitcloud: commits synchronized
  finished in * (glob)

  $ cd ..

  $ cd client2
  $ cat >> .sl/config << EOF
  > [commitcloud]
  > updateonmove=true
  > EOF
  $ sl up 41f3b9359864
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ sl cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: nothing to upload
  pulling 8134e74ecdc8 from ssh://user@dummy/server
  searching for changes
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
  $ sl up 41f3b9359864 -q --config checkout.obsolete-mode=ignore --config commit.modify-obsolete-mode=ignore
  $ echo 1 > filea.txt && sl addremove && sl amend -m "`sl descr | head -n1` amended"
  adding filea.txt
  warning: changing an old version of a commit will diverge your stack:
  - 41f3b9359864 -> 8134e74ecdc8 (amend)
  $ sl id -i
  abd5311ab3c6
  $ sl up 41f3b9359864 -q --config checkout.obsolete-mode=ignore --config commit.modify-obsolete-mode=ignore
  $ echo 1 > fileb.txt && sl addremove && sl amend -m "`sl descr | head -n1` amended"
  adding fileb.txt
  warning: changing an old version of a commit will diverge your stack:
  - 41f3b9359864 -> 8134e74ecdc8 (amend)
  - 41f3b9359864 -> abd5311ab3c6 (amend)
  $ sl id -i
  cebbb614447e
  $ sl cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: head 'abd5311ab3c6' hasn't been uploaded yet
  commitcloud: head 'cebbb614447e' hasn't been uploaded yet
  edenapi: queue 2 commits for upload
  edenapi: queue 0 files for upload
  edenapi: queue 2 trees for upload
  edenapi: uploaded 2 trees
  edenapi: uploaded 2 changesets
  commitcloud: commits synchronized
  finished in * (glob)

  $ cd ..

  $ cd client2
  $ sl up 41f3b9359864 -q --config checkout.obsolete-mode=ignore --config commit.modify-obsolete-mode=ignore
  $ sl cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: nothing to upload
  pulling abd5311ab3c6 cebbb614447e from ssh://user@dummy/server
  searching for changes
  commitcloud: commits synchronized
  finished in * (glob)
  commitcloud: current revision 41f3b9359864 has been replaced remotely with multiple revisions
  (run 'sl goto HASH' to go to the desired revision)
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
  $ sl up abd5311ab3c6 -q
  $ echo 2 >> filea.txt && sl amend -m "`sl descr | head -n1` amended"
  $ sl id -i
  f4ea578a3184
  $ echo 3 >> filea.txt && sl amend -m "`sl descr | head -n1` amended"
  $ sl id -i
  acf8d3fd70ac
  $ echo 4 >> filea.txt && sl amend -m "`sl descr | head -n1` amended"
  $ sl id -i
  fada67350ab0
  $ sl cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: head 'fada67350ab0' hasn't been uploaded yet
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  commitcloud: commits synchronized
  finished in * (glob)

  $ cd ..

  $ cd client2
  $ sl up abd5311ab3c6 -q
  $ sl cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: nothing to upload
  pulling fada67350ab0 from ssh://user@dummy/server
  searching for changes
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
  $ sl up cebbb614447e -q
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
  
  $ sl rebase -s cebbb614447e -d d20a80d4def3 -m "`sl descr | head -n1` rebased" --collapse
  rebasing cebbb614447e "commit2 amended amended"
  $ echo 5 >> filea.txt && sl amend -m "`sl descr | head -n1` amended"
  $ sl id -i
  99e818be5af0
  $ sl rebase -s 99e818be5af0 -d a7bb357e7299 -m "`sl descr | head -n1` rebased" --collapse
  rebasing 99e818be5af0 "commit2 amended amended rebased amended"
  $ echo 6 >> filea.txt && sl amend -m "`sl descr | head -n1` amended"
  $ tglog -r '.'
  @  68e035cc1996 'commit2 amended amended rebased amended rebased amended'
  │
  ~
  $ sl cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: head '68e035cc1996' hasn't been uploaded yet
  edenapi: queue 1 commit for upload
  edenapi: queue 0 files for upload
  edenapi: queue 0 trees for upload
  edenapi: uploaded 1 changeset
  commitcloud: commits synchronized
  finished in * (glob)

  $ cd ..

  $ cd client2
  $ sl up cebbb614447e -q
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
  
  $ sl cloud sync -q
  $ tglog -r '.'
  @  68e035cc1996 'commit2 amended amended rebased amended rebased amended'
  │
  ~

Clean up by hiding some commits, and create a new stack

  $ sl goto d20a80d4def3 -q
  $ sl hide a7bb357e7299
  hiding commit a7bb357e7299 "commit1 amended"
  hiding commit 8134e74ecdc8 "commit2 amended amended"
  hiding commit fada67350ab0 "commit2 amended amended amended amended amended"
  hiding commit 68e035cc1996 "commit2 amended amended rebased amended rebased am"
  4 changesets hidden
  removing bookmark 'bookmark1' (was at: 8134e74ecdc8)
  removing bookmark 'bookmark1-testhost' (was at: a7bb357e7299)
  2 bookmarks removed
  $ sl bookmark testbookmark
  $ sl cloud sync -q
  $ mkcommit "stack commit 1"
  $ mkcommit "stack commit 2"
  $ sl cloud sync -q
  $ cd ..
  $ cd client1
  $ sl goto d20a80d4def3 -q
  $ sl cloud sync -q
  $ tglog
  o  f2ccc2716735 'stack commit 2' testbookmark
  │
  o  74473a0f136f 'stack commit 1'
  │
  @  d20a80d4def3 'base'
  
Test race between cloud sync and another transaction

  $ sl next -q
  [74473a] stack commit 1

  $ sl amend -m "race attempt" --no-rebase
  hint[amend-restack]: descendants of 74473a0f136f are left behind - use 'sl restack' to rebase them
  hint[hint-ack]: use 'sl hint --ack amend-restack' to silence these hints

Create a simultanous rebase and cloud sync, where the cloud sync has won the
race for the lock

  $ touch $TESTTMP/lockdelay
  $ HGPREWLOCKFILE=$TESTTMP/lockdelay sl rebase --restack --config extensions.lockdelay=$TESTDIR/lockdelay.py >$TESTTMP/racerebase.out 2>&1 &
  $ HGPOSTLOCKFILE=$TESTTMP/lockdelay sl cloud sync -q --config extensions.lockdelay=$TESTDIR/lockdelay.py >$TESTTMP/racecloudsync.out 2>&1 &
  $ sleep 1

Let them both run together
  $ rm $TESTTMP/lockdelay

Wait for them to complete and then do another cloud sync
  $ wait
  $ sl cloud sync -q
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
  $ sl cloud sync -q
  $ tglog
  @  715c1454ae33 'stack commit 2' testbookmark
  │
  o  4b4f26511f8b 'race attempt'
  │
  o  d20a80d4def3 'base'
  
  $ cd ..

Test interactions with  share extension

Create a shared client directory

  $ sl share client1 client1b
  updating working directory
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat shared.rc >> client1b/.sl/config
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

  $ sl cloud check
  2c0ce859e76ae60f6f832279c75fae4d61da6be2 not backed up
  $ sl cloud sync -q
  $ sl cloud check
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
  
  $ sl cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: nothing to upload
  commitcloud: commits synchronized
  finished in * (glob)

Check cloud sync pulls in the shared commit in the other client

  $ cd ../client2
  $ sl cloud sync -q
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
  $ sl cloud sync --workspace-version 1
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: this version has been already synchronized

Check '--check-autosync-enabled' option
  $ sl cloud sync --check-autosync-enabled
  commitcloud: background operations are currently disabled
  $ sl cloud sync --check-autosync-enabled --config infinitepushbackup.autobackup=true
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: nothing to upload
  commitcloud: commits synchronized
  finished in * (glob)

Check '--raw-workspace-name' option
  $ sl cloud sync --raw-workspace-name 'user/test/default'
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: nothing to upload
  commitcloud: commits synchronized
  finished in 0.00 sec
  $ sl cloud sync --raw-workspace-name 'user/test/other'
  current workspace is different than the workspace to sync
  [1]

Check handling of failures
Simulate failure to backup a commit by setting a FAILPOINT.

  $ sl up testbookmark
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark testbookmark)
  $ sl metaedit -r 2c0ce859e76a -m 'shared commit updated'
  $ mkcommit toobig
  $ sl book toobig
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
  
  $ FAILPOINTS="eagerepo::api::uploadchangesets=return(error)" sl cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: head 'a6b97eebbf74' hasn't been uploaded yet
  commitcloud: head '9bd68ef10d6b' hasn't been uploaded yet
  edenapi: queue 2 commits for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  abort: server responded 500 Internal Server Error for eager://$TESTTMP/server/upload_changesets: failpoint. Headers: {}
  [255]

Run cloud status after failing to synchronize
  $ sl cloud status
  Workspace: default
  Raw Workspace Name: user/test/default
  Automatic Sync (on local changes): OFF
  Automatic Sync via 'Scm Daemon' (on remote changes): OFF
  Last Sync Version: * (glob)
  Last Sync Heads: 1 (0 omitted)
  Last Sync Bookmarks: 1 (0 omitted)
  Last Sync Remote Bookmarks: 1
  Last Sync Time: * (glob)
  Last Sync Status: Exception:
  server responded 500 Internal Server Error for eager://$TESTTMP/server/upload_changesets: failpoint. Headers: {}

  $ sl cloud check -r .
  9bd68ef10d6bdb8ebf3273a7b91bc4f3debe2a87 not backed up

Sync in the other repo and check it still looks ok (but with the failed commits missing).

  $ cd ../client1
  $ sl cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: nothing to upload
  commitcloud: commits synchronized
  finished in * (glob)
  $ tglog
  o  2c0ce859e76a 'shared commit'
  │
  o  715c1454ae33 'stack commit 2' testbookmark
  │
  @  4b4f26511f8b 'race attempt'
  │
  o  d20a80d4def3 'base'

Now sync in the repo we failed in.  This time it should work.

  $ cd ../client2
  $ sl cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: head 'a6b97eebbf74' hasn't been uploaded yet
  commitcloud: head '9bd68ef10d6b' hasn't been uploaded yet
  edenapi: queue 2 commits for upload
  edenapi: queue 0 files for upload
  edenapi: queue 0 trees for upload
  edenapi: uploaded 2 changesets
  commitcloud: commits synchronized
  finished in * (glob)
  $ sl cloud check -r .
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
  $ sl cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: nothing to upload
  pulling a6b97eebbf74 9bd68ef10d6b from ssh://user@dummy/server
  searching for changes
  commitcloud: commits synchronized
  finished in * (glob)
  $ tglog
  o  9bd68ef10d6b 'toobig' testbookmark toobig
  │
  │ o  a6b97eebbf74 'shared commit updated'
  ├─╯
  o  715c1454ae33 'stack commit 2'
  │
  @  4b4f26511f8b 'race attempt'
  │
  o  d20a80d4def3 'base'
Clean up

  $ sl up -q -r d20a80d4def38df63a4b330b7fb688f3d4cae1e3
  $ sl hide -q 4b4f26511f8b
  $ cd ..
  $ fullsync client1 client2
  $ cd client2
  $ sl up -q -r d20a80d4def38df63a4b330b7fb688f3d4cae1e3
  $ cd ../client1

Make two stacks

  $ mkcommit 'stack 1 first'
  $ mkcommit 'stack 1 second'
  $ sl up -q -r d20a80d4def38df63a4b330b7fb688f3d4cae1e3
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


This test is disabled to unblock migration to eagerepo as server. Marking e58a6603d256 as
public causes sl to "pull" with e58a6603d256 as "common" even though the server doesn't
know e58a6603d256. Currently, Mononoke (and presumably legacy repos) ignores unknown
commits, but eagerepo errors out. Probably it should be treated as an error in Mononoke
as well.
#if false
Make one of the commits public when it shouldn't be.

  $ sl debugmakepublic e58a6603d256
  $ sl cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: head '9a3e7907fd5c' hasn't been uploaded yet
  commitcloud: head '799d22972c4e' hasn't been uploaded yet
  edenapi: queue 3 commits for upload
  edenapi: queue 3 files for upload
  edenapi: uploaded 3 files
  edenapi: queue 3 trees for upload
  edenapi: uploaded 3 trees
  edenapi: uploaded 2 changesets
  commitcloud: failed to synchronize 9a3e7907fd5c
  finished in 0.00 sec

Commit still becomes available in the other repo

  $ cd ../client2
  $ sl cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: nothing to upload
  pulling e58a6603d256 799d22972c4e from ssh://user@dummy/server
  searching for changes
  commitcloud: commits synchronized
  finished in * (glob)
  $ tglog
  o  799d22972c4e 'stack 2 second'
  │
  o  3597ff85ead0 'stack 2 first'
  │
  @  d20a80d4def3 'base'
Fix up that public commit, set it back to draft
  $ cd ../client1
  $ sl debugmakepublic -d -r e58a6603d256
#endif

Make a public commit and put two bookmarks on it
  $ cd ../server
  $ mkcommit 'public'
  $ sl bookmark publicbookmark2

Pull it into one of the clients and rebase one of the stacks onto it
  $ cd ../client1
  $ sl pull -q
  $ sl trglog
  @  799d22972c4e 'stack 2 second'
  │
  o  3597ff85ead0 'stack 2 first'
  │
  │ o  9a3e7907fd5c 'stack 1 second'
  │ │
  │ o  e58a6603d256 'stack 1 first'
  ├─╯
  │ o  acd5b9e8c656 'public'  remote/publicbookmark1 remote/publicbookmark2
  ├─╯
  o  d20a80d4def3 'base'
  $ sl rebase -s e58a6603d256 -d publicbookmark1
  rebasing e58a6603d256 "stack 1 first"
  rebasing 9a3e7907fd5c "stack 1 second"
  $ sl cloud sync -q

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
  $ sl cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: nothing to upload
  pulling publicbookmark1 (d20a80d4def3 -> acd5b9e8c656) via fast path
  imported commit graph for 1 commit(s) (1 segment(s))
  pulling 799d22972c4e 2da6c73964b8 from ssh://user@dummy/server
  searching for changes
  commitcloud: commits synchronized
  finished in * (glob)
  $ sl trglog
  o  2da6c73964b8 'stack 1 second'
  │
  o  5df7c1d8d8ab 'stack 1 first'
  │
  │ o  799d22972c4e 'stack 2 second'
  │ │
  │ o  3597ff85ead0 'stack 2 first'
  │ │
  o │  acd5b9e8c656 'public'  remote/publicbookmark1 remote/publicbookmark2
  ├─╯
  @  d20a80d4def3 'base'
Do a pull on this client.  The remote bookmarks now get updated.
  $ sl pull
  pulling from ssh://user@dummy/server
  searching for changes
  $ sl trglog
  o  97250524560a 'public 2'  remote/publicbookmark2
  │
  │ o  2da6c73964b8 'stack 1 second'
  │ │
  │ o  5df7c1d8d8ab 'stack 1 first'
  ├─╯
  │ o  799d22972c4e 'stack 2 second'
  │ │
  │ o  3597ff85ead0 'stack 2 first'
  │ │
  o │  acd5b9e8c656 'public'  remote/publicbookmark1
  ├─╯
  @  d20a80d4def3 'base'
Rebase the commits again, and resync to the first client.
  $ sl rebase -s 5df7c1d8d8ab -d publicbookmark2
  rebasing 5df7c1d8d8ab "stack 1 first"
  rebasing 2da6c73964b8 "stack 1 second"
  $ sl cloud sync -q
  $ cd ../client1
  $ sl cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: nothing to upload
  pulling 97250524560a af621240884f from ssh://user@dummy/server
  searching for changes
  commitcloud: commits synchronized
  finished in * (glob)
  $ sl trglog
  o  af621240884f 'stack 1 second'
  │
  o  81cd67693e59 'stack 1 first'
  │
  o  97250524560a 'public 2'  remote/publicbookmark2
  │
  │ @  799d22972c4e 'stack 2 second'
  │ │
  │ o  3597ff85ead0 'stack 2 first'
  │ │
  o │  acd5b9e8c656 'public'  remote/publicbookmark1
  ├─╯
  o  d20a80d4def3 'base'
A final pull gets everything in sync here, too.
  $ sl pull -q
  $ sl trglog
  o  af621240884f 'stack 1 second'
  │
  o  81cd67693e59 'stack 1 first'
  │
  o  97250524560a 'public 2'  remote/publicbookmark2
  │
  │ @  799d22972c4e 'stack 2 second'
  │ │
  │ o  3597ff85ead0 'stack 2 first'
  │ │
  o │  acd5b9e8c656 'public'  remote/publicbookmark1
  ├─╯
  o  d20a80d4def3 'base'
Check subscription when join/leave and also scm service health check
  $ cat >> .sl/config << EOF
  > [commitcloud]
  > subscription_enabled = true
  > subscriber_service_tcp_port = 15432
  > connected_subscribers_path = $TESTTMP
  > EOF
  $ sl cloud sync -q
  $ cat $TESTTMP/.commitcloud/joined/*
  [commitcloud]
  workspace=user/test/default
  repo_name=server
  repo_root=$TESTTMP/client1/.sl
  $ sl cloud leave
  commitcloud: this repository is now disconnected from the 'user/test/default' workspace
  $ ls $TESTTMP/.commitcloud/joined/
  $ sl cloud join -q
  $ cat $TESTTMP/.commitcloud/joined/*
  [commitcloud]
  workspace=user/test/default
  repo_name=server
  repo_root=$TESTTMP/client1/.sl

  $ cd ..

Rejoin
  $ rm -rf client2
  $ mkdir client2

  $ mkdir $TESTTMP/otherservicelocation
  $ mkdir $TESTTMP/othertokenlocation

  $ sl clone ssh://user@dummy/server client2 -q
  $ cd client2
  $ cat ../shared.rc >> .sl/config

Reconnect to a service where the workspace is brand new.  This should work.

  $ sl cloud reconnect --config "commitcloud.servicelocation=$TESTTMP/otherservicelocation"
  commitcloud: attempting to connect to the 'user/test/default' workspace for the 'server' repo
  commitcloud: this repository is now connected to the 'user/test/default' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: nothing to upload
  commitcloud: commits synchronized
  finished in * (glob)

  $ sl cloud disconnect
  commitcloud: this repository is now disconnected from the 'user/test/default' workspace

Reconnect to the default repository.  This should work and pull in the commits.
  $ sl cloud reconnect
  commitcloud: attempting to connect to the 'user/test/default' workspace for the 'server' repo
  commitcloud: this repository is now connected to the 'user/test/default' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: nothing to upload
  pulling 799d22972c4e af621240884f from ssh://user@dummy/server
  searching for changes
  commitcloud: commits synchronized
  finished in * (glob)
  hint[commitcloud-switch]: the following commitcloud workspaces (backups) are available for this repo:
  user/test/feature
  user/test/default
  run `sl cloud list` inside the repo to see all your workspaces,
  find the one the repo is connected to and learn how to switch between them
  hint[hint-ack]: use 'sl hint --ack commitcloud-switch' to silence these hints

  $ sl trglog
  o  af621240884f 'stack 1 second'
  │
  o  81cd67693e59 'stack 1 first'
  │
  │ o  799d22972c4e 'stack 2 second'
  │ │
  │ o  3597ff85ead0 'stack 2 first'
  │ │
  o │  97250524560a 'public 2'  remote/publicbookmark2
  │ │
  @ │  acd5b9e8c656 'public'  remote/publicbookmark1
  ├─╯
  o  d20a80d4def3 'base'

Reconnecting while already connected does nothing.
  $ sl cloud reconnect

  $ sl cloud disconnect
  commitcloud: this repository is now disconnected from the 'user/test/default' workspace

Completely remove commit cloud config and then pull with automigrate enabled.
This should also reconnect.

  $ rm .sl/store/commitcloudrc
  $ sl pull --config commitcloud.automigrate=true
  pulling from ssh://user@dummy/server
  imported commit graph for 0 commits (0 segments)
Enable commit cloud background operations.
  $ setconfig infinitepushbackup.autobackup=true
  $ sl pull --config commitcloud.automigrate=true
  pulling from ssh://user@dummy/server
  imported commit graph for 0 commits (0 segments)
  commitcloud: attempting to connect to the 'user/test/default' workspace for the 'server' repo
  commitcloud: this repository is now connected to the 'user/test/default' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: nothing to upload
  commitcloud: commits synchronized
  finished in * (glob)
  hint[commitcloud-switch]: the following commitcloud workspaces (backups) are available for this repo:
  user/test/feature
  user/test/default
  run `sl cloud list` inside the repo to see all your workspaces,
  find the one the repo is connected to and learn how to switch between them
  hint[hint-ack]: use 'sl hint --ack commitcloud-switch' to silence these hints

But not if already connected.
  $ sl pull --config commitcloud.automigrate=true
  pulling from ssh://user@dummy/server
  imported commit graph for 0 commits (0 segments)

  $ sl cloud disconnect
  commitcloud: this repository is now disconnected from the 'user/test/default' workspace

And not if explicitly disconnected.
  $ sl pull --config commitcloud.automigrate=true
  pulling from ssh://user@dummy/server
  imported commit graph for 0 commits (0 segments)

Pull with automigrate enabled and host-specific workspaces

  $ rm .sl/store/commitcloudrc
  $ sl pull --config commitcloud.automigrate=true --config commitcloud.automigratehostworkspace=true
  pulling from ssh://user@dummy/server
  imported commit graph for 0 commits (0 segments)
  commitcloud: attempting to connect to the 'user/test/testhost' workspace for the 'server' repo
  commitcloud: this repository is now connected to the 'user/test/testhost' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/testhost'
  commitcloud: nothing to upload
  commitcloud: commits synchronized
  finished in * (glob)

Host-specific workspace now exists, so it should be chosen as the one to connect to
  $ sl cloud leave
  commitcloud: this repository is now disconnected from the 'user/test/testhost' workspace
  $ sl cloud reconnect
  commitcloud: attempting to connect to the 'user/test/testhost' workspace for the 'server' repo
  commitcloud: this repository is now connected to the 'user/test/testhost' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/testhost'
  commitcloud: nothing to upload
  commitcloud: commits synchronized
  finished in * (glob)
  hint[commitcloud-switch]: the following commitcloud workspaces (backups) are available for this repo:
  user/test/feature
  user/test/default
  user/test/testhost
  run `sl cloud list` inside the repo to see all your workspaces,
  find the one the repo is connected to and learn how to switch between them
  hint[hint-ack]: use 'sl hint --ack commitcloud-switch' to silence these hints

  $ sl cloud join --switch -w default --traceback
  commitcloud: synchronizing 'server' with 'user/test/testhost'
  commitcloud: nothing to upload
  commitcloud: commits synchronized
  finished in * (glob)
  commitcloud: now this repository will be switched from the 'user/test/testhost' to the 'user/test/default' workspace
  commitcloud: this repository is now connected to the 'user/test/default' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: nothing to upload
  commitcloud: commits synchronized
  finished in 0.00 sec
