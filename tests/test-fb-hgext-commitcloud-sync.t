  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > fbamend =
  > infinitepush =
  > commitcloud =
  > [ui]
  > ssh = python "$TESTDIR/dummyssh"
  > [infinitepush]
  > branchpattern = re:scratch/.*
  > [commitcloud]
  > hostname = testhost
  > [alias]
  > tglog = log -G --template "{node|short} '{desc}' {bookmarks}\n"
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

Make the first clone of the server
  $ hg clone ssh://user@dummy/server client1 -q
  $ cd client1
  $ cat >> .hg/hgrc << EOF
  > [commitcloud]
  > servicetype = local
  > servicelocation = $TESTTMP
  > EOF
  $ cd ..

Make the second clone of the server
  $ hg clone ssh://user@dummy/server client2 -q
  $ cd client2
  $ cat >> .hg/hgrc << EOF
  > [commitcloud]
  > servicetype = local
  > servicelocation = $TESTTMP
  > EOF
  $ cd ..

Make a commit in the first client, and sync it
  $ cd client1
  $ mkcommit "commit1"
  $ hg pushbackup -q
  $ hg cloudsync
  $ cd ..

Sync from the second client - the commit should appear
  $ cd client2
  $ hg cloudsync
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets fa5d62c46fd7
  (run 'hg update' to get a working copy)

  $ hg up -q tip
  $ hg tglog
  @  fa5d62c46fd7 'commit1'
  |
  o  d20a80d4def3 'base'
  
Make a commit from the second client and sync it
  $ mkcommit "commit2"
  $ hg pushbackup -q
  $ hg cloudsync
  $ cd ..

On the first client, make a bookmark, then sync - the bookmark and new commit should be synced
  $ cd client1
  $ hg bookmark -r 0 bookmark1
  $ hg cloudsync
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 2 files
  new changesets 02f6fc2b7154
  (run 'hg update' to get a working copy)
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
  pulling from ssh://user@dummy/server
  no changes found
  adding changesets
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 2 files
  $ hg tglog
  @  02f6fc2b7154 'commit2'
  |
  o  fa5d62c46fd7 'commit1'
  |
  o  d20a80d4def3 'base' bookmark1
  
Move the bookmark on the second client, and then sync it
  $ hg bookmark -r 2 -f bookmark1
  $ hg cloudsync
  $ cd ..

Move the bookmark also on the first client, it should be forked in the sync
  $ cd client1
  $ hg bookmark -r 1 -f bookmark1
  $ hg cloudsync
  pulling from ssh://user@dummy/server
  no changes found
  adding changesets
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 2 files
  bookmark1 changed locally and remotely, local bookmark renamed to bookmark1-testhost
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
  $ hg amend --rebase -m "commit1 amended"
  rebasing 2:02f6fc2b7154 "commit2" (bookmark1)
  $ hg pushbackup -q
  $ hg cloudsync
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
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 1 changes to 2 files (+1 heads)
  new changesets a7bb357e7299:48610b1a7ec0
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hg up -q tip
  $ hg tglog
  @  48610b1a7ec0 'commit2' bookmark1
  |
  o  a7bb357e7299 'commit1 amended' bookmark1-testhost
  |
  o  d20a80d4def3 'base'
  
  $ test ! -f .hg/store/commitcloudpendingobsmarkers
