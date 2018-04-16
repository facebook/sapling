  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > fbamend =
  > infinitepush =
  > commitcloud =
  > rebase =
  > [ui]
  > ssh = python "$TESTDIR/dummyssh"
  > [infinitepush]
  > branchpattern = re:scratch/.*
  > [commitcloud]
  > hostname = testhost
  > [alias]
  > tglog = log -G --template "{node|short} '{desc}' {bookmarks}\n"
  > descr = log -r '.' --template "{desc}"
  > [experimental]
  > evolution = createmarkers, allowunstable
  > EOF

  $ mkcommit() {
  >   echo "$1" > "$1"
  >   hg commit -Aqm "$1"
  > }

Full sync for repo1 and repo2 in quiet mode
This means cloudsync in the repo1, cloudsync in the repo2 and then again in the repo1
To be run if some test require full sync state before the test
  $ fullsync() {
  >   cd "$1"
  >   hg cloudsync -q
  >   cd ../"$2"
  >   hg cloudsync -q
  >   cd ../"$1"
  >   hg cloudsync -q
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
Joining before registration:
  $ hg cloudjoin
  abort: #commitcloud registration error: please run `hg cloudregister` before joining a workspace
  authentication instructions:
  visit htts://localhost/oauth to generate a registration token
  please contact The Test Team @ FB for more information
  [255]
Registration:
  $ hg cloudregister
  #commitcloud welcome to registration!
  abort: #commitcloud registration error: token is not provided and not found
  authentication instructions:
  visit htts://localhost/oauth to generate a registration token
  please contact The Test Team @ FB for more information
  [255]
  $ hg cloudregister -t xxxxxx
  #commitcloud welcome to registration!
  registration successful
  $ hg cloudregister -t xxxxxx --config "commitcloud.user_token_path=$TESTTMP/somedir"
  #commitcloud welcome to registration!
  abort: #commitcloud unexpected configuration error: invalid commitcloud.user_token_path '$TESTTMP/somedir'
  please contact The Test Team @ FB to report misconfiguration
  [255]
Joining:
  $ hg cloudsync
  abort: #commitcloud workspace error: undefined workspace
  your repo is not connected to any workspace
  please run `hg cloudjoin --help` for more details
  [255]
  $ hg cloudjoin
  #commitcloud this repository is now part of the 'user/test/default' workspace for the 'server' repo

  $ cd ..

Make the second clone of the server
  $ hg clone ssh://user@dummy/server client2 -q
  $ cd client2
  $ cat ../shared.rc >> .hg/hgrc
Registration:
  $ hg cloudregister
  #commitcloud welcome to registration!
  you have been already registered
  $ hg cloudregister -t yyyyy
  #commitcloud welcome to registration!
  your token will be updated
  registration successful
Joining:
  $ hg cloudjoin
  #commitcloud this repository is now part of the 'user/test/default' workspace for the 'server' repo

  $ cd ..

Make a commit in the first client, and sync it
  $ cd client1
  $ mkcommit "commit1"
  $ hg cloudsync
  #commitcloud start synchronization
  remote: pushing 1 commit:
  remote:     fa5d62c46fd7  commit1
  #commitcloud cloudsync done
  $ cd ..

Sync from the second client - the commit should appear
  $ cd client2
  $ hg cloudsync
  #commitcloud start synchronization
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets fa5d62c46fd7
  (run 'hg update' to get a working copy)
  #commitcloud cloudsync done

  $ hg up -q tip
  $ hg tglog
  @  fa5d62c46fd7 'commit1'
  |
  o  d20a80d4def3 'base'
  
Make a commit from the second client and sync it
  $ mkcommit "commit2"
  $ hg cloudsync
  #commitcloud start synchronization
  remote: pushing 2 commits:
  remote:     fa5d62c46fd7  commit1
  remote:     02f6fc2b7154  commit2
  #commitcloud cloudsync done
  $ cd ..

On the first client, make a bookmark, then sync - the bookmark and new commit should be synced
  $ cd client1
  $ hg bookmark -r 0 bookmark1
  $ hg cloudsync
  #commitcloud start synchronization
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 2 files
  new changesets 02f6fc2b7154
  (run 'hg update' to get a working copy)
  #commitcloud cloudsync done
  $ hg tglog
  o  02f6fc2b7154 'commit2'
  |
  @  fa5d62c46fd7 'commit1'
  |
  o  d20a80d4def3 'base' bookmark1
  
  $ cd ..

Sync the bookmark back to the second client
  $ cd client2
  $ hg cloudsync
  #commitcloud start synchronization
  #commitcloud cloudsync done
  $ hg tglog
  @  02f6fc2b7154 'commit2'
  |
  o  fa5d62c46fd7 'commit1'
  |
  o  d20a80d4def3 'base' bookmark1
  
Move the bookmark on the second client, and then sync it
  $ hg bookmark -r 2 -f bookmark1
  $ hg cloudsync
  #commitcloud start synchronization
  #commitcloud cloudsync done

  $ cd ..

Move the bookmark also on the first client, it should be forked in the sync
  $ cd client1
  $ hg bookmark -r 1 -f bookmark1
  $ hg cloudsync
  #commitcloud start synchronization
  bookmark1 changed locally and remotely, local bookmark renamed to bookmark1-testhost
  #commitcloud cloudsync done
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
  $ hg cloudsync
  #commitcloud start synchronization
  remote: pushing 2 commits:
  remote:     a7bb357e7299  commit1 amended
  remote:     48610b1a7ec0  commit2
  #commitcloud cloudsync done
  $ hg tglog
  o  48610b1a7ec0 'commit2' bookmark1
  |
  @  a7bb357e7299 'commit1 amended' bookmark1-testhost
  |
  o  d20a80d4def3 'base'
  
  $ cd ..

Sync the amended commit to the other client
  $ cd client2
  $ hg cloudsync
  #commitcloud start synchronization
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 1 changes to 2 files (+1 heads)
  new changesets a7bb357e7299:48610b1a7ec0
  (run 'hg heads' to see heads, 'hg merge' to merge)
  #commitcloud cloudsync done
  #commitcloud current revision 02f6fc2b7154 has been moved remotely to 48610b1a7ec0
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
  $ hg cloudsync 2>&1
  abort: #commitcloud invalid workspace data: 'failed to parse commitcloudstate.usertestdefault.b6eca'
  please run `hg cloudrecover`
  [255]
  $ hg cloudrecover
  #commitcloud start recovering
  #commitcloud start synchronization
  #commitcloud cloudsync done

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
  $ hg cloudsync
  #commitcloud start synchronization
  remote: pushing 2 commits:
  remote:     a7bb357e7299  commit1 amended
  remote:     41f3b9359864  commit2 amended
  #commitcloud cloudsync done

  $ cd ..

  $ cd client2
  $ hg cloudsync
  #commitcloud start synchronization
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 2 files (+1 heads)
  new changesets 41f3b9359864
  (run 'hg heads' to see heads, 'hg merge' to merge)
  #commitcloud cloudsync done
  #commitcloud current revision 48610b1a7ec0 has been moved remotely to 41f3b9359864
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
  $ hg cloudsync
  #commitcloud start synchronization
  remote: pushing 2 commits:
  remote:     a7bb357e7299  commit1 amended
  remote:     8134e74ecdc8  commit2 amended amended
  #commitcloud cloudsync done

  $ cd ..

  $ cd client2
  $ cat >> .hg/hgrc << EOF
  > [commitcloud]
  > updateonmove=true
  > EOF
  $ hg up 41f3b9359864
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg cloudsync
  #commitcloud start synchronization
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 3 files (+1 heads)
  new changesets 8134e74ecdc8
  (run 'hg heads' to see heads, 'hg merge' to merge)
  #commitcloud cloudsync done
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
  $ hg cloudsync
  #commitcloud start synchronization
  remote: pushing 3 commits:
  remote:     a7bb357e7299  commit1 amended
  remote:     abd5311ab3c6  commit2 amended amended
  remote:     cebbb614447e  commit2 amended amended
  #commitcloud cloudsync done

  $ cd ..

  $ cd client2
  $ hg up 41f3b9359864 -q
  $ hg cloudsync
  #commitcloud start synchronization
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
  #commitcloud cloudsync done
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
  $ hg cloudsync
  #commitcloud start synchronization
  remote: pushing 2 commits:
  remote:     a7bb357e7299  commit1 amended
  remote:     fada67350ab0  commit2 amended amended amended amended amended
  #commitcloud cloudsync done

  $ cd ..

  $ cd client2
  $ hg up abd5311ab3c6 -q
  $ hg cloudsync
  #commitcloud start synchronization
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 3 files (+1 heads)
  new changesets fada67350ab0
  (run 'hg heads .' to see heads, 'hg merge' to merge)
  #commitcloud cloudsync done
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
  $ hg cloudsync
  #commitcloud start synchronization
  remote: pushing 2 commits:
  remote:     a7bb357e7299  commit1 amended
  remote:     68e035cc1996  commit2 amended amended rebased amended rebased am
  #commitcloud cloudsync done

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
  
  $ hg cloudsync -q
  $ hg tglog -r '.'
  @  68e035cc1996 'commit2 amended amended rebased amended rebased amended'
  |
  ~
