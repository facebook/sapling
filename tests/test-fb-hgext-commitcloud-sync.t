  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > fbamend =
  > infinitepush =
  > infinitepushbackup =
  > commitcloud =
  > rebase =
  > remotenames =
  > share =
  > [ui]
  > ssh = python "$TESTDIR/dummyssh"
  > [infinitepush]
  > branchpattern = re:scratch/.*
  > [commitcloud]
  > hostname = testhost
  > [alias]
  > tglog = log -G --template "{node|short} '{desc}' {bookmarks}\n"
  > trglog = log -G --template "{node|short} '{desc}' {bookmarks} {remotenames}\n"
  > descr = log -r '.' --template "{desc}"
  > [experimental]
  > evolution = createmarkers, allowunstable
  > EOF

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

  $ hg init server
  $ cd server
  $ cat >> .hg/hgrc << EOF
  > [infinitepush]
  > server = yes
  > indextype = disk
  > storetype = disk
  > reponame = testrepo
  > EOF

  $ mkcommit "base"
  $ cd ..

Make shared part of config
  $ cat >> shared.rc << EOF
  > [commitcloud]
  > servicetype = local
  > servicelocation = $TESTTMP
  > user_token_path = $TESTTMP
  > auth_help = visit htts://localhost/oauth to generate a registration token
  > owner_team = The Test Team @ FB
  > EOF

Make the first clone of the server
  $ hg clone ssh://user@dummy/server client1 -q
  $ cd client1
  $ cat ../shared.rc >> .hg/hgrc
Registration:
  $ hg cloud auth
  abort: #commitcloud registration error: authentication with commit cloud required
  authentication instructions:
  visit htts://localhost/oauth to generate a registration token
  please contact The Test Team @ FB for more information
  (use 'hg cloud auth --token TOKEN' to set a token)
  [255]
  $ hg cloud auth -t xxxxxx
  setting authentication token
  authentication successful
  $ hg cloud auth -t xxxxxx --config "commitcloud.user_token_path=$TESTTMP/somedir"
  abort: #commitcloud unexpected configuration error: invalid commitcloud.user_token_path '$TESTTMP/somedir'
  please contact The Test Team @ FB to report misconfiguration
  [255]
Joining:
  $ hg cloud sync
  abort: #commitcloud workspace error: undefined workspace
  your repo is not connected to any workspace
  use 'hg cloud join --help' for more details
  [255]
  $ hg cloud join
  #commitcloud this repository is now connected to the 'user/test/default' workspace for the 'server' repo
  #commitcloud synchronizing 'server' with 'user/test/default'
  #commitcloud commits synchronized

  $ cd ..

Make the second clone of the server
  $ hg clone ssh://user@dummy/server client2 -q
  $ cd client2
  $ cat ../shared.rc >> .hg/hgrc
Registration:
  $ hg cloud auth
  using existing authentication token
  authentication successful
  $ hg cloud auth -t yyyyy
  updating authentication token
  authentication successful
Joining:
  $ hg cloud join
  #commitcloud this repository is now connected to the 'user/test/default' workspace for the 'server' repo
  #commitcloud synchronizing 'server' with 'user/test/default'
  #commitcloud commits synchronized

  $ cd ..

Make a commit in the first client, and sync it
  $ cd client1
  $ mkcommit "commit1"
  $ hg cloud sync
  #commitcloud synchronizing 'server' with 'user/test/default'
  backing up stack rooted at fa5d62c46fd7
  remote: pushing 1 commit:
  remote:     fa5d62c46fd7  commit1
  #commitcloud commits synchronized
  $ cd ..

Sync from the second client - the commit should appear
  $ cd client2
  $ hg cloud sync
  #commitcloud synchronizing 'server' with 'user/test/default'
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets fa5d62c46fd7
  (run 'hg update' to get a working copy)
  #commitcloud commits synchronized

  $ hg up -q tip
  $ hg tglog
  @  fa5d62c46fd7 'commit1'
  |
  o  d20a80d4def3 'base'
  
Make a commit from the second client and sync it
  $ mkcommit "commit2"
  $ hg cloud sync
  #commitcloud synchronizing 'server' with 'user/test/default'
  backing up stack rooted at fa5d62c46fd7
  remote: pushing 2 commits:
  remote:     fa5d62c46fd7  commit1
  remote:     02f6fc2b7154  commit2
  #commitcloud commits synchronized
  $ cd ..

On the first client, make a bookmark, then sync - the bookmark and new commit should be synced
  $ cd client1
  $ hg bookmark -r 0 bookmark1
  $ hg cloud sync
  #commitcloud synchronizing 'server' with 'user/test/default'
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 2 files
  new changesets 02f6fc2b7154
  (run 'hg update' to get a working copy)
  #commitcloud commits synchronized
  $ hg tglog
  o  02f6fc2b7154 'commit2'
  |
  @  fa5d62c46fd7 'commit1'
  |
  o  d20a80d4def3 'base' bookmark1
  
  $ cd ..

Sync the bookmark back to the second client
  $ cd client2
  $ hg cloud sync
  #commitcloud synchronizing 'server' with 'user/test/default'
  #commitcloud commits synchronized
  $ hg tglog
  @  02f6fc2b7154 'commit2'
  |
  o  fa5d62c46fd7 'commit1'
  |
  o  d20a80d4def3 'base' bookmark1
  
Move the bookmark on the second client, and then sync it
  $ hg bookmark -r 2 -f bookmark1
  $ hg cloud sync
  #commitcloud synchronizing 'server' with 'user/test/default'
  #commitcloud commits synchronized

  $ cd ..

Move the bookmark also on the first client, it should be forked in the sync
  $ cd client1
  $ hg bookmark -r 1 -f bookmark1
  $ hg cloud sync
  #commitcloud synchronizing 'server' with 'user/test/default'
  bookmark1 changed locally and remotely, local bookmark renamed to bookmark1-testhost
  #commitcloud commits synchronized
  $ hg tglog
  o  02f6fc2b7154 'commit2' bookmark1
  |
  @  fa5d62c46fd7 'commit1' bookmark1-testhost
  |
  o  d20a80d4def3 'base'
  
  $ cd ..

Amend a commit
  $ cd client1
  $ echo more >> commit1
  $ hg amend --rebase -m "`hg descr | head -n1` amended"
  rebasing 2:02f6fc2b7154 "commit2" (bookmark1)
  $ hg cloud sync
  #commitcloud synchronizing 'server' with 'user/test/default'
  backing up stack rooted at a7bb357e7299
  remote: pushing 2 commits:
  remote:     a7bb357e7299  commit1 amended
  remote:     48610b1a7ec0  commit2
  #commitcloud commits synchronized
  $ hg tglog
  o  48610b1a7ec0 'commit2' bookmark1
  |
  @  a7bb357e7299 'commit1 amended' bookmark1-testhost
  |
  o  d20a80d4def3 'base'
  
  $ cd ..

Sync the amended commit to the other client
  $ cd client2
  $ hg cloud sync
  #commitcloud synchronizing 'server' with 'user/test/default'
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 1 changes to 2 files (+1 heads)
  obsoleted 2 changesets
  new changesets a7bb357e7299:48610b1a7ec0
  (run 'hg heads' to see heads, 'hg merge' to merge)
  #commitcloud commits synchronized
  #commitcloud current revision 02f6fc2b7154 has been moved remotely to 48610b1a7ec0
  hint[commitcloud-update-on-move]: if you would like to update to the moved version automatically add
  [commitcloud]
  updateonmove = true
  to your .hgrc config file
  hint[hint-ack]: use 'hg hint --ack commitcloud-update-on-move' to silence these hints
  $ hg up -q tip
  $ hg tglog
  @  48610b1a7ec0 'commit2' bookmark1
  |
  o  a7bb357e7299 'commit1 amended' bookmark1-testhost
  |
  o  d20a80d4def3 'base'
  
  $ test ! -f .hg/store/commitcloudpendingobsmarkers

  $ cd ..

Test recovery from broken state (example: invalid json)
  $ cd client1
  $ echo '}}}' >> .hg/store/commitcloudstate.usertestdefault.b6eca
  $ hg cloud sync 2>&1
  #commitcloud synchronizing 'server' with 'user/test/default'
  abort: #commitcloud invalid workspace data: 'failed to parse commitcloudstate.usertestdefault.b6eca'
  please run 'hg cloud recover'
  [255]
  $ hg cloud recover
  #commitcloud clearing local commit cloud cache
  #commitcloud synchronizing 'server' with 'user/test/default'
  #commitcloud commits synchronized

  $ cd ..
  $ fullsync client1 client2

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
  #commitcloud synchronizing 'server' with 'user/test/default'
  backing up stack rooted at a7bb357e7299
  remote: pushing 2 commits:
  remote:     a7bb357e7299  commit1 amended
  remote:     41f3b9359864  commit2 amended
  #commitcloud commits synchronized

  $ cd ..

  $ cd client2
  $ hg cloud sync
  #commitcloud synchronizing 'server' with 'user/test/default'
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 2 files (+1 heads)
  obsoleted 1 changesets
  new changesets 41f3b9359864
  (run 'hg heads' to see heads, 'hg merge' to merge)
  #commitcloud commits synchronized
  #commitcloud current revision 48610b1a7ec0 has been moved remotely to 41f3b9359864
  hint[commitcloud-update-on-move]: if you would like to update to the moved version automatically add
  [commitcloud]
  updateonmove = true
  to your .hgrc config file
  hint[hint-ack]: use 'hg hint --ack commitcloud-update-on-move' to silence these hints
  $ hg tglog
  o  41f3b9359864 'commit2 amended' bookmark1
  |
  | @  48610b1a7ec0 'commit2'
  |/
  o  a7bb357e7299 'commit1 amended' bookmark1-testhost
  |
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
  #commitcloud synchronizing 'server' with 'user/test/default'
  backing up stack rooted at a7bb357e7299
  remote: pushing 2 commits:
  remote:     a7bb357e7299  commit1 amended
  remote:     8134e74ecdc8  commit2 amended amended
  #commitcloud commits synchronized

  $ cd ..

  $ cd client2
  $ cat >> .hg/hgrc << EOF
  > [commitcloud]
  > updateonmove=true
  > EOF
  $ hg up 41f3b9359864
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg cloud sync
  #commitcloud synchronizing 'server' with 'user/test/default'
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 3 files (+1 heads)
  obsoleted 1 changesets
  new changesets 8134e74ecdc8
  (run 'hg heads' to see heads, 'hg merge' to merge)
  #commitcloud commits synchronized
  #commitcloud current revision 41f3b9359864 has been moved remotely to 8134e74ecdc8
  updating to 8134e74ecdc8
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg tglog
  @  8134e74ecdc8 'commit2 amended amended' bookmark1
  |
  o  a7bb357e7299 'commit1 amended' bookmark1-testhost
  |
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
  #commitcloud synchronizing 'server' with 'user/test/default'
  backing up stack rooted at a7bb357e7299
  remote: pushing 3 commits:
  remote:     a7bb357e7299  commit1 amended
  remote:     abd5311ab3c6  commit2 amended amended
  remote:     cebbb614447e  commit2 amended amended
  #commitcloud commits synchronized

  $ cd ..

  $ cd client2
  $ hg up 41f3b9359864 -q
  $ hg cloud sync
  #commitcloud synchronizing 'server' with 'user/test/default'
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 3 files (+1 heads)
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 3 files (+1 heads)
  new changesets abd5311ab3c6:cebbb614447e
  (run 'hg heads .' to see heads, 'hg merge' to merge)
  #commitcloud commits synchronized
  #commitcloud current revision 41f3b9359864 has been replaced remotely with multiple revisions
  Please run `hg update` to go to the desired revision
  $ hg tglog
  o  cebbb614447e 'commit2 amended amended'
  |
  | o  abd5311ab3c6 'commit2 amended amended'
  |/
  | o  8134e74ecdc8 'commit2 amended amended' bookmark1
  |/
  | @  41f3b9359864 'commit2 amended'
  |/
  o  a7bb357e7299 'commit1 amended' bookmark1-testhost
  |
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
  #commitcloud synchronizing 'server' with 'user/test/default'
  backing up stack rooted at a7bb357e7299
  remote: pushing 2 commits:
  remote:     a7bb357e7299  commit1 amended
  remote:     fada67350ab0  commit2 amended amended amended amended amended
  #commitcloud commits synchronized

  $ cd ..

  $ cd client2
  $ hg up abd5311ab3c6 -q
  $ hg cloud sync
  #commitcloud synchronizing 'server' with 'user/test/default'
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 3 files (+1 heads)
  obsoleted 1 changesets
  new changesets fada67350ab0
  (run 'hg heads .' to see heads, 'hg merge' to merge)
  #commitcloud commits synchronized
  #commitcloud current revision abd5311ab3c6 has been moved remotely to fada67350ab0
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
  $ hg tglog
  o  fada67350ab0 'commit2 amended amended amended amended amended'
  |
  | @  cebbb614447e 'commit2 amended amended'
  |/
  | o  8134e74ecdc8 'commit2 amended amended' bookmark1
  |/
  o  a7bb357e7299 'commit1 amended' bookmark1-testhost
  |
  o  d20a80d4def3 'base'
  
  $ hg rebase -s cebbb614447e -d d20a80d4def3 -m "`hg descr | head -n1` rebased" --collapse
  rebasing 8:cebbb614447e "commit2 amended amended"
  $ echo 5 >> filea.txt && hg amend -m "`hg descr | head -n1` amended"
  $ hg id -i
  99e818be5af0
  $ hg rebase -s 99e818be5af0 -d a7bb357e7299 -m "`hg descr | head -n1` rebased" --collapse
  rebasing 13:99e818be5af0 "commit2 amended amended rebased amended" (tip)
  $ echo 6 >> filea.txt && hg amend -m "`hg descr | head -n1` amended"
  $ hg tglog -r '.'
  @  68e035cc1996 'commit2 amended amended rebased amended rebased amended'
  |
  ~
  $ hg cloud sync
  #commitcloud synchronizing 'server' with 'user/test/default'
  backing up stack rooted at a7bb357e7299
  remote: pushing 2 commits:
  remote:     a7bb357e7299  commit1 amended
  remote:     68e035cc1996  commit2 amended amended rebased amended rebased am
  #commitcloud commits synchronized

  $ cd ..

  $ cd client2
  $ hg up cebbb614447e -q
  $ hg tglog
  o  fada67350ab0 'commit2 amended amended amended amended amended'
  |
  | @  cebbb614447e 'commit2 amended amended'
  |/
  | o  8134e74ecdc8 'commit2 amended amended' bookmark1
  |/
  | x  41f3b9359864 'commit2 amended'
  |/
  o  a7bb357e7299 'commit1 amended' bookmark1-testhost
  |
  o  d20a80d4def3 'base'
  
  $ hg cloud sync -q
  $ hg tglog -r '.'
  @  68e035cc1996 'commit2 amended amended rebased amended rebased amended'
  |
  ~

Clean up by hiding some commits, and create a new stack

  $ hg update d20a80d4def3 -q
  $ hg hide a7bb357e7299
  hiding commit a7bb357e7299 "commit1 amended"
  hiding commit 41f3b9359864 "commit2 amended"
  hiding commit 8134e74ecdc8 "commit2 amended amended"
  hiding commit fada67350ab0 "commit2 amended amended amended amended amended"
  hiding commit 68e035cc1996 "commit2 amended amended rebased amended rebased am"
  5 changesets hidden
  removing bookmark "bookmark1 (was at: 8134e74ecdc8)"
  removing bookmark "bookmark1-testhost (was at: a7bb357e7299)"
  2 bookmarks removed
  $ hg bookmark testbookmark
  $ hg cloud sync -q
  $ mkcommit "stack commit 1"
  $ mkcommit "stack commit 2"
  $ hg cloud sync -q
  $ cd ..
  $ cd client1
  $ hg update d20a80d4def3 -q
  $ hg cloud sync -q
  $ hg tglog
  o  f2ccc2716735 'stack commit 2' testbookmark
  |
  o  74473a0f136f 'stack commit 1'
  |
  @  d20a80d4def3 'base'
  
Test race between syncing obsmarkers and a transaction creating new ones

Create an extension that runs a restack command while we're syncing

  $ cat > $TESTTMP/syncrace.py << EOF
  > from mercurial import extensions
  > from hgext.commitcloud import service
  > def extsetup(ui):
  >     def wrapget(orig, *args, **kwargs):
  >         serv = orig(*args, **kwargs)
  >         class WrappedService(serv.__class__):
  >             def updatereferences(self, *args, **kwargs):
  >                 res = super(WrappedService, self).updatereferences(*args, **kwargs)
  >                 ui.system('hg rebase --restack')
  >                 return res
  >         serv.__class__ = WrappedService
  >         return serv
  >     extensions.wrapfunction(service, 'get', wrapget)
  > EOF

  $ hg next -q
  [74473a] stack commit 1
  $ hg amend -m "race attempt" --no-rebase
  hint[amend-restack]: descendants of 74473a0f136f are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints
  $ hg cloud sync -q --config extensions.syncrace=$TESTTMP/syncrace.py
  rebasing 17:f2ccc2716735 "stack commit 2" (testbookmark)
  $ hg cloud sync -q
  $ hg tglog
  o  715c1454ae33 'stack commit 2' testbookmark
  |
  @  4b4f26511f8b 'race attempt'
  |
  o  d20a80d4def3 'base'
  
  $ cd ..
  $ cd client2
  $ hg cloud sync -q
  $ hg tglog
  @  715c1454ae33 'stack commit 2' testbookmark
  |
  o  4b4f26511f8b 'race attempt'
  |
  o  d20a80d4def3 'base'
  
  $ cd ..

Test interactions with  share extension

Create a shared client directory

  $ hg share client1 client1b
  updating working directory
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat shared.rc >> client1b/.hg/hgrc
  $ cd client1b
  $ hg tglog
  @  715c1454ae33 'stack commit 2'
  |
  o  4b4f26511f8b 'race attempt'
  |
  o  d20a80d4def3 'base'
  
Make a new commit to be shared

  $ mkcommit "shared commit"
  $ hg tglog
  @  2c0ce859e76a 'shared commit'
  |
  o  715c1454ae33 'stack commit 2'
  |
  o  4b4f26511f8b 'race attempt'
  |
  o  d20a80d4def3 'base'
  
Check cloud sync backs up the commit

  $ hg isbackedup
  2c0ce859e76ae60f6f832279c75fae4d61da6be2 not backed up
  $ hg cloud sync -q
  $ hg isbackedup
  2c0ce859e76ae60f6f832279c75fae4d61da6be2 backed up

Check cloud sync in the source repo doesn't need to do anything

  $ cd ../client1
  $ hg tglog
  o  2c0ce859e76a 'shared commit'
  |
  o  715c1454ae33 'stack commit 2' testbookmark
  |
  @  4b4f26511f8b 'race attempt'
  |
  o  d20a80d4def3 'base'
  
  $ hg cloud sync
  #commitcloud synchronizing 'server' with 'user/test/default'
  #commitcloud commits synchronized

Check cloud sync pulls in the shared commit in the other client

  $ cd ../client2
  $ hg cloud sync -q
  $ hg tglog
  o  2c0ce859e76a 'shared commit'
  |
  @  715c1454ae33 'stack commit 2' testbookmark
  |
  o  4b4f26511f8b 'race attempt'
  |
  o  d20a80d4def3 'base'
  
Check '--workspace_version' option
  $ hg cloud sync --workspace-version 1
  #commitcloud synchronizing 'server' with 'user/test/default'
  #commitcloud this version has been already synchronized

Check '--check_autosync_enabled' option
  $ hg cloud sync --check-autosync-enabled
  #commitcloud automatic backup and synchronization is currently disabled
  $ hg backupdisable
  note: background backup was already disabled
  background backup is now disabled until * (glob)
  $ hg cloud sync --check-autosync-enabled
  #commitcloud automatic backup and synchronization is currently disabled
  $ hg backupenable
  background backup is enabled
  $ hg cloud sync
  #commitcloud synchronizing 'server' with 'user/test/default'
  #commitcloud commits synchronized

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
  $ hg tglog
  @  9bd68ef10d6b 'toobig' testbookmark toobig
  |
  | o  a6b97eebbf74 'shared commit updated'
  |/
  o  715c1454ae33 'stack commit 2'
  |
  o  4b4f26511f8b 'race attempt'
  |
  o  d20a80d4def3 'base'
  
  $ hg cloud sync
  #commitcloud synchronizing 'server' with 'user/test/default'
  backing up stack rooted at 4b4f26511f8b
  remote: pushing 4 commits:
  remote:     4b4f26511f8b  race attempt
  remote:     715c1454ae33  stack commit 2
  remote:     a6b97eebbf74  shared commit updated
  remote:     9bd68ef10d6b  toobig
  push failed: bundle is too big: 2219 bytes. max allowed size is 0 MB
  retrying push with discovery
  searching for changes
  remote: pushing 4 commits:
  remote:     4b4f26511f8b  race attempt
  remote:     715c1454ae33  stack commit 2
  remote:     a6b97eebbf74  shared commit updated
  remote:     9bd68ef10d6b  toobig
  push of stack 4b4f26511f8b failed: bundle is too big: 2219 bytes. max allowed size is 0 MB
  retrying each head individually
  remote: pushing 3 commits:
  remote:     4b4f26511f8b  race attempt
  remote:     715c1454ae33  stack commit 2
  remote:     a6b97eebbf74  shared commit updated
  push failed: bundle is too big: 1738 bytes. max allowed size is 0 MB
  retrying push with discovery
  searching for changes
  remote: pushing 3 commits:
  remote:     4b4f26511f8b  race attempt
  remote:     715c1454ae33  stack commit 2
  remote:     a6b97eebbf74  shared commit updated
  push of head a6b97eebbf74 failed: bundle is too big: 1738 bytes. max allowed size is 0 MB
  remote: pushing 3 commits:
  remote:     4b4f26511f8b  race attempt
  remote:     715c1454ae33  stack commit 2
  remote:     9bd68ef10d6b  toobig
  push failed: bundle is too big: 1695 bytes. max allowed size is 0 MB
  retrying push with discovery
  searching for changes
  remote: pushing 3 commits:
  remote:     4b4f26511f8b  race attempt
  remote:     715c1454ae33  stack commit 2
  remote:     9bd68ef10d6b  toobig
  push of head 9bd68ef10d6b failed: bundle is too big: 1695 bytes. max allowed size is 0 MB
  abort: #commitcloud failed to synchronize commits: '2 heads could not be pushed'
  please retry 'hg cloud sync'
  please contact The Test Team @ FB if this error persists
  [255]

  $ hg isbackedup -r .
  9bd68ef10d6bdb8ebf3273a7b91bc4f3debe2a87 not backed up

Set the limit back high.  Sync in the other repo and check it still looks ok
(but with the failed commits missing).

  $ mv $TESTTMP/server-hgrc.bak ../server/.hg/hgrc
  $ cd ../client1
  $ hg cloud sync
  #commitcloud synchronizing 'server' with 'user/test/default'
  #commitcloud commits synchronized
  $ hg tglog
  o  715c1454ae33 'stack commit 2' testbookmark
  |
  @  4b4f26511f8b 'race attempt'
  |
  o  d20a80d4def3 'base'
  

Now sync in the repo we failed in.  This time it should work.

  $ cd ../client2
  $ hg cloud sync
  #commitcloud synchronizing 'server' with 'user/test/default'
  backing up stack rooted at 4b4f26511f8b
  remote: pushing 4 commits:
  remote:     4b4f26511f8b  race attempt
  remote:     715c1454ae33  stack commit 2
  remote:     a6b97eebbf74  shared commit updated
  remote:     9bd68ef10d6b  toobig
  #commitcloud commits synchronized
  $ hg isbackedup -r .
  9bd68ef10d6bdb8ebf3273a7b91bc4f3debe2a87 backed up
  $ hg tglog
  @  9bd68ef10d6b 'toobig' testbookmark toobig
  |
  | o  a6b97eebbf74 'shared commit updated'
  |/
  o  715c1454ae33 'stack commit 2'
  |
  o  4b4f26511f8b 'race attempt'
  |
  o  d20a80d4def3 'base'
  

And the commits should now be availble in the other client.

  $ cd ../client1
  $ hg cloud sync
  #commitcloud synchronizing 'server' with 'user/test/default'
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 3 files (+1 heads)
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 3 files (+1 heads)
  new changesets a6b97eebbf74:9bd68ef10d6b
  (run 'hg heads .' to see heads, 'hg merge' to merge)
  #commitcloud commits synchronized
  $ hg tglog
  o  9bd68ef10d6b 'toobig' testbookmark toobig
  |
  | o  a6b97eebbf74 'shared commit updated'
  |/
  o  715c1454ae33 'stack commit 2'
  |
  @  4b4f26511f8b 'race attempt'
  |
  o  d20a80d4def3 'base'
  
Clean up

  $ hg up -q -r 0
  $ hg hide -q 4b4f26511f8b
  $ cd ..
  $ fullsync client1 client2
  $ cd client2
  $ hg up -q -r 0
  $ cd ../client1

Make two stacks

  $ mkcommit 'stack 1 first'
  $ mkcommit 'stack 1 second'
  $ hg up -q -r 0
  $ mkcommit 'stack 2 first'
  $ mkcommit 'stack 2 second'
  $ hg tglog
  @  799d22972c4e 'stack 2 second'
  |
  o  3597ff85ead0 'stack 2 first'
  |
  | o  9a3e7907fd5c 'stack 1 second'
  | |
  | o  e58a6603d256 'stack 1 first'
  |/
  o  d20a80d4def3 'base'
  
Make one of the commits public when it shouldn't be.

  $ hg phase -p e58a6603d256
  $ hg cloud sync
  #commitcloud synchronizing 'server' with 'user/test/default'
  backing up stack rooted at 9a3e7907fd5c
  remote: abort: 00changelog.i@e58a6603d256: unknown parent!
  push failed: stream ended unexpectedly (got 0 bytes, expected 4)
  retrying push with discovery
  searching for changes
  remote: pushing 2 commits:
  remote:     e58a6603d256  stack 1 first
  remote:     9a3e7907fd5c  stack 1 second
  backing up stack rooted at 3597ff85ead0
  remote: pushing 2 commits:
  remote:     3597ff85ead0  stack 2 first
  remote:     799d22972c4e  stack 2 second
  #commitcloud commits synchronized

Commit still becomes available in the other repo

  $ cd ../client2
  $ hg cloud sync
  #commitcloud synchronizing 'server' with 'user/test/default'
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 3 changes to 3 files (+1 heads)
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files (+1 heads)
  new changesets e58a6603d256:799d22972c4e
  (run 'hg heads .' to see heads, 'hg merge' to merge)
  #commitcloud commits synchronized
  $ hg tglog
  o  799d22972c4e 'stack 2 second'
  |
  o  3597ff85ead0 'stack 2 first'
  |
  | o  9a3e7907fd5c 'stack 1 second'
  | |
  | o  e58a6603d256 'stack 1 first'
  |/
  @  d20a80d4def3 'base'
  
Fix up that public commit, set it back to draft
  $ cd ../client1
  $ hg phase -fd e58a6603d256

Make a public commit and put two bookmarks on it
  $ cd ../server
  $ mkcommit 'public'
  $ hg bookmark publicbookmark1
  $ hg bookmark publicbookmark2

Pull it into one of the clients and rebase one of the stacks onto it
  $ cd ../client1
  $ hg pull -q
  $ hg trglog
  o  acd5b9e8c656 'public'  default/publicbookmark1 default/publicbookmark2 default/default
  |
  | @  799d22972c4e 'stack 2 second'
  | |
  | o  3597ff85ead0 'stack 2 first'
  |/
  | o  9a3e7907fd5c 'stack 1 second'
  | |
  | o  e58a6603d256 'stack 1 first'
  |/
  o  d20a80d4def3 'base'
  
  $ hg rebase -s e58a6603d256 -d publicbookmark1
  rebasing 23:e58a6603d256 "stack 1 first"
  rebasing 24:9a3e7907fd5c "stack 1 second"
  $ hg cloud sync -q

Create another public commit on the server, moving one of the bookmarks
  $ cd ../server
  $ mkcommit 'public 2'
  $ hg tglog
  @  97250524560a 'public 2' publicbookmark2
  |
  o  acd5b9e8c656 'public' publicbookmark1
  |
  o  d20a80d4def3 'base'
  
Sync this onto the second client, the remote bookmarks don't change.
  $ cd ../client2
  $ hg cloud sync
  #commitcloud synchronizing 'server' with 'user/test/default'
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 0 changes to 3 files
  obsoleted 2 changesets
  new changesets acd5b9e8c656:2da6c73964b8
  (run 'hg heads' to see heads, 'hg merge' to merge)
  #commitcloud commits synchronized
  $ hg trglog
  o  2da6c73964b8 'stack 1 second'
  |
  o  5df7c1d8d8ab 'stack 1 first'
  |
  o  acd5b9e8c656 'public'
  |
  | o  799d22972c4e 'stack 2 second'
  | |
  | o  3597ff85ead0 'stack 2 first'
  |/
  @  d20a80d4def3 'base'  default/default
  
Do a pull on this client.  The remote bookmarks now get updated.
  $ hg pull
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  new changesets 97250524560a
  (run 'hg heads .' to see heads, 'hg merge' to merge)
  $ hg trglog
  o  97250524560a 'public 2'  default/publicbookmark2 default/default
  |
  | o  2da6c73964b8 'stack 1 second'
  | |
  | o  5df7c1d8d8ab 'stack 1 first'
  |/
  o  acd5b9e8c656 'public'  default/publicbookmark1
  |
  | o  799d22972c4e 'stack 2 second'
  | |
  | o  3597ff85ead0 'stack 2 first'
  |/
  @  d20a80d4def3 'base'
  
Rebase the commits again, and resync to the first client.
  $ hg rebase -s 5df7c1d8d8ab -d publicbookmark2
  rebasing 23:5df7c1d8d8ab "stack 1 first"
  rebasing 24:2da6c73964b8 "stack 1 second"
  $ hg cloud sync -q
  $ cd ../client1
  $ hg cloud sync
  #commitcloud synchronizing 'server' with 'user/test/default'
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 0 changes to 3 files
  obsoleted 2 changesets
  new changesets 97250524560a:af621240884f
  (run 'hg heads' to see heads, 'hg merge' to merge)
  #commitcloud commits synchronized
  $ hg trglog
  o  af621240884f 'stack 1 second'
  |
  o  81cd67693e59 'stack 1 first'
  |
  o  97250524560a 'public 2'
  |
  o  acd5b9e8c656 'public'  default/publicbookmark1 default/publicbookmark2 default/default
  |
  | @  799d22972c4e 'stack 2 second'
  | |
  | o  3597ff85ead0 'stack 2 first'
  |/
  o  d20a80d4def3 'base'
  
A final pull gets everything in sync here, too.
  $ hg pull -q
  $ hg trglog
  o  af621240884f 'stack 1 second'
  |
  o  81cd67693e59 'stack 1 first'
  |
  o  97250524560a 'public 2'  default/publicbookmark2 default/default
  |
  o  acd5b9e8c656 'public'  default/publicbookmark1
  |
  | @  799d22972c4e 'stack 2 second'
  | |
  | o  3597ff85ead0 'stack 2 first'
  |/
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
  #commitcloud this repository is now disconnected from commit cloud
  $ ls $TESTTMP/.commitcloud/joined/
  $ hg cloud join -q
  $ cat $TESTTMP/.commitcloud/joined/*
  [commitcloud]
  workspace=user/test/default
  repo_name=server
  repo_root=$TESTTMP/client1/.hg

