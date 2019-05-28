  $ setconfig extensions.treemanifest=!
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > amend =
  > commitcloud =
  > infinitepush =
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
  > trglog = log -G --template "{node|short} '{desc}' {bookmarks} {remotenames}\n"
  > descr = log -r '.' --template "{desc}"
  > [experimental]
  > evolution = createmarkers, allowunstable
  > EOF

  $ setconfig remotefilelog.reponame=server

  $ mkcommit() {
  >   echo "$1" > "$1"
  >   hg commit -Aqm "$1"
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
  > owner_team = The Test Team @ FB
  > EOF

Make the first clone of the server
  $ hg clone ssh://user@dummy/server client1 -q
  $ cd client1
  $ cat ../shared.rc >> .hg/hgrc
  $ hg cloud auth -t xxxxxx -q
  $ hg cloud join -q

  $ cd ..

Make the second clone of the server
  $ hg clone ssh://user@dummy/server client2 -q
  $ cd client2
  $ cat ../shared.rc >> .hg/hgrc
  $ hg cloud auth -q
  $ hg cloud join -q

  $ cd ..

Test for `hg unamend`

Make a commit in the first client, and sync it
  $ cd client1
  $ mkcommit "feature1"
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  backing up stack rooted at 1cf4a5a0e8fc
  remote: pushing 1 commit:
  remote:     1cf4a5a0e8fc  feature1
  commitcloud: commits synchronized
  finished in * (glob)

  $ hg amend -m "feature1 renamed"
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  backing up stack rooted at b68dd726c6c6
  remote: pushing 1 commit:
  remote:     b68dd726c6c6  feature1 renamed
  commitcloud: commits synchronized
  finished in * (glob)

  $ cd ..

Sync from the second client and `hg unamend` there
  $ cd client2
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  pulling b68dd726c6c6
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets b68dd726c6c6
  commitcloud: commits synchronized
  finished in * (glob)

  $ tglog
  o  1: b68dd726c6c6 'feature1 renamed'
  |
  @  0: d20a80d4def3 'base'
  

  $ hg up b68dd726c6c6
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved


  $ hg unamend --config extensions.commitcloud=!
  abort: unknown revision '1cf4a5a0e8fc41ef1289e833ebdb22d754c080ac'!
  [255]


  $ hg unamend
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 1 files (+1 heads)

  $ tglog
  @  2: 1cf4a5a0e8fc 'feature1'
  |
  o  0: d20a80d4def3 'base'
  

  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: commits synchronized
  finished in * (glob)

  $ cd ..

  $ cd client1

  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: commits synchronized
  finished in * (glob)
  commitcloud: current revision b68dd726c6c6 has been moved remotely to 1cf4a5a0e8fc
  hint[commitcloud-update-on-move]: if you would like to update to the moved version automatically add
  [commitcloud]
  updateonmove = true
  to your .hgrc config file
  hint[hint-ack]: use 'hg hint --ack commitcloud-update-on-move' to silence these hints
  $ tglog
  @  2: b68dd726c6c6 'feature1 renamed'
  |
  | o  1: 1cf4a5a0e8fc 'feature1'
  |/
  o  0: d20a80d4def3 'base'
  
Amend twice, unamend, then unhide.  This causes a cycle in the obsgraph.
  $ hg up -q 1cf4a5a0e8fc
  $ hg amend -m "feature1 renamed2"
  $ hg amend -m "feature1 renamed3"
  $ hg unamend
  $ hg unhide 74b668b6b779
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  backing up stack rooted at cb45bbd0ae75
  remote: pushing 1 commit:
  remote:     cb45bbd0ae75  feature1 renamed2
  backing up stack rooted at 74b668b6b779
  remote: pushing 1 commit:
  remote:     74b668b6b779  feature1 renamed3
  commitcloud: commits synchronized
  finished in * sec (glob)
  commitcloud: current revision cb45bbd0ae75 has been replaced remotely with multiple revisions
  (run 'hg update HASH' to go to the desired revision)

Now cloud sync in the other client.  The cycle means we can't reliably pick a destination.
  $ cd ../client2
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  pulling cb45bbd0ae75 74b668b6b779
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 1 files (+1 heads)
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 1 files (+1 heads)
  new changesets cb45bbd0ae75:74b668b6b779
  commitcloud: commits synchronized
  finished in * sec (glob)
  commitcloud: current revision 1cf4a5a0e8fc has been replaced remotely with multiple revisions
  (run 'hg update HASH' to go to the desired revision)
