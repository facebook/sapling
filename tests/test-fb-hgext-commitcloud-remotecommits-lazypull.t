  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > amend =
  > commitcloud =
  > infinitepush =
  > infinitepushbackup =
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
  #commitcloud synchronizing 'server' with 'user/test/default'
  backing up stack rooted at 1cf4a5a0e8fc
  remote: pushing 1 commit:
  remote:     1cf4a5a0e8fc  feature1
  #commitcloud commits synchronized

  $ hg amend -m "feature1 renamed"
  $ hg cloud sync
  #commitcloud synchronizing 'server' with 'user/test/default'
  backing up stack rooted at b68dd726c6c6
  remote: pushing 1 commit:
  remote:     b68dd726c6c6  feature1 renamed
  #commitcloud commits synchronized

  $ cd ..

Sync from the second client and `hg unamend` there
  $ cd client2
  $ hg cloud sync
  #commitcloud synchronizing 'server' with 'user/test/default'
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets b68dd726c6c6
  (run 'hg update' to get a working copy)
  #commitcloud commits synchronized

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
  (run 'hg heads' to see heads, 'hg merge' to merge)

  $ tglog
  @  2: 1cf4a5a0e8fc 'feature1'
  |
  o  0: d20a80d4def3 'base'
  

  $ hg cloud sync
  #commitcloud synchronizing 'server' with 'user/test/default'
  backing up stack rooted at 1cf4a5a0e8fc
  remote: pushing 1 commit:
  remote:     1cf4a5a0e8fc  feature1
  #commitcloud commits synchronized
  obs-cycle detected (happens for "divergence" cases like A obsoletes B; B obsoletes A)
  #commitcloud current revision 1cf4a5a0e8fc has been replaced remotely with multiple revisions
  Please run `hg update` to go to the desired revision

  $ cd ..

  $ cd client1

  $ hg cloud sync
  #commitcloud synchronizing 'server' with 'user/test/default'
  #commitcloud commits synchronized
  obs-cycle detected (happens for "divergence" cases like A obsoletes B; B obsoletes A)
  #commitcloud current revision b68dd726c6c6 has been replaced remotely with multiple revisions
  Please run `hg update` to go to the desired revision
  $ tglog
  @  2: b68dd726c6c6 'feature1 renamed'
  |
  | o  1: 1cf4a5a0e8fc 'feature1'
  |/
  o  0: d20a80d4def3 'base'
  
