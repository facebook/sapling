#chg-compatible

  $ setconfig extensions.treemanifest=!
  $ enable amend directaccess commitcloud infinitepush rebase remotenames undo
  $ setconfig ui.ssh="python \"$TESTDIR/dummyssh\""
  $ setconfig infinitepush.branchpattern="re:scratch/.*"
  $ setconfig commitcloud.hostname=testhost
  $ setconfig visibility.enabled=true
  $ setconfig experimental.evolution=
  $ setconfig experimental.narrow-heads=true
  $ setconfig mutation.record=true mutation.enabled=true mutation.user=test mutation.date="0 0"
  $ setconfig remotefilelog.reponame=server
  $ setconfig hint.ack='*'

  $ newrepo server
  $ setconfig infinitepush.server=yes infinitepush.indextype=disk infinitepush.storetype=disk infinitepush.reponame=testrepo
  $ echo base > base
  $ hg commit -Aqm base
  $ hg bookmark master

Create a client with some initial commits and sync them to the cloud workspace.

  $ cd $TESTTMP
  $ hg clone ssh://user@dummy/server client1 -q
  $ cd client1
  $ setconfig commitcloud.servicetype=local commitcloud.servicelocation=$TESTTMP
  $ setconfig commitcloud.user_token_path=$TESTTMP
  $ hg cloud auth -t xxxxxx
  setting authentication token
  authentication successful
  $ hg cloud join
  commitcloud: this repository is now connected to the 'user/test/default' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: commits synchronized
  finished in * sec (glob)
  $ drawdag << EOS
  > B D    # amend: A -> C -> E
  > | |    # rebase: B -> D
  > A C E
  >  \|/
  >   Z
  >   |
  > d20a80d4def3
  > EOS
  $ tglogm
  o  6: c70a9bd6bfd1 'E'
  |
  | o  5: 6ba5de8abe43 'D'
  | |
  | x  4: 2d0f0af04f18 'C'  (Rewritten using amend into c70a9bd6bfd1)
  |/
  o  1: dae3b312bb78 'Z'
  |
  @  0: d20a80d4def3 'base'
  
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  backing up stack rooted at dae3b312bb78
  remote: pushing 4 commits:
  remote:     dae3b312bb78  Z
  remote:     2d0f0af04f18  C
  remote:     6ba5de8abe43  D
  remote:     c70a9bd6bfd1  E
  commitcloud: commits synchronized
  finished in * sec (glob)

Create another client and use it to modify the commits and create some new ones.

  $ cd $TESTTMP
  $ hg clone ssh://user@dummy/server client2 -q
  $ cd client2
  $ setconfig commitcloud.servicetype=local commitcloud.servicelocation=$TESTTMP
  $ setconfig commitcloud.user_token_path=$TESTTMP
  $ hg cloud auth -t xxxxxx
  updating authentication token
  authentication successful
  $ hg cloud join
  commitcloud: this repository is now connected to the 'user/test/default' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/default'
  pulling 6ba5de8abe43 c70a9bd6bfd1
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 3 files
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 2 files
  commitcloud: commits synchronized
  finished in * sec (glob)
  $ tglogm
  o  4: c70a9bd6bfd1 'E'
  |
  | o  3: 6ba5de8abe43 'D'
  | |
  | x  2: 2d0f0af04f18 'C'  (Rewritten using amend into c70a9bd6bfd1)
  |/
  o  1: dae3b312bb78 'Z'
  |
  @  0: d20a80d4def3 'base'
  

  $ hg rebase -r $D -d $E
  rebasing 6ba5de8abe43 "D"
  $ hg up -q $Z
  $ echo X > X
  $ hg commit -Aqm X
  $ tglogm
  @  6: dd114d9b2f9e 'X'
  |
  | o  5: d8fc5ae9b7ef 'D'
  | |
  | o  4: c70a9bd6bfd1 'E'
  |/
  o  1: dae3b312bb78 'Z'
  |
  o  0: d20a80d4def3 'base'
  
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  backing up stack rooted at dae3b312bb78
  remote: pushing 4 commits:
  remote:     dae3b312bb78  Z
  remote:     c70a9bd6bfd1  E
  remote:     d8fc5ae9b7ef  D
  remote:     dd114d9b2f9e  X
  commitcloud: commits synchronized
  finished in * sec (glob)

Before syncing, create a new commit in the original client

  $ cd $TESTTMP/client1
  $ hg up -q $E
  $ echo F > F
  $ hg commit -Aqm F

Also introduce some divergence by rebasing the same commit

  $ hg rebase -r $D -d $Z
  rebasing 6ba5de8abe43 "D"

Now cloud sync.  The sets of commits should be merged.

  $ tglogm
  o  8: 6caded0e9807 'D'
  |
  | @  7: ba83c5428cb2 'F'
  | |
  | o  6: c70a9bd6bfd1 'E'
  |/
  o  1: dae3b312bb78 'Z'
  |
  o  0: d20a80d4def3 'base'
  
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  backing up stack rooted at dae3b312bb78
  remote: pushing 4 commits:
  remote:     dae3b312bb78  Z
  remote:     c70a9bd6bfd1  E
  remote:     ba83c5428cb2  F
  remote:     6caded0e9807  D
  pulling d8fc5ae9b7ef dd114d9b2f9e
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 3 files
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 2 files
  commitcloud: commits synchronized
  finished in * sec (glob)
  $ tglogm
  o  10: dd114d9b2f9e 'X'
  |
  | o  9: d8fc5ae9b7ef 'D'
  | |
  +---o  8: 6caded0e9807 'D'
  | |
  | | @  7: ba83c5428cb2 'F'
  | |/
  | o  6: c70a9bd6bfd1 'E'
  |/
  o  1: dae3b312bb78 'Z'
  |
  o  0: d20a80d4def3 'base'
  

Cloud sync back to the other client, it should get the same smartlog (apart from ordering).

  $ cd $TESTTMP/client2
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  pulling ba83c5428cb2 6caded0e9807
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 3 files
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 2 files
  commitcloud: commits synchronized
  finished in * sec (glob)
  $ tglogm
  o  8: 6caded0e9807 'D'
  |
  | o  7: ba83c5428cb2 'F'
  | |
  +---@  6: dd114d9b2f9e 'X'
  | |
  | | o  5: d8fc5ae9b7ef 'D'
  | |/
  | o  4: c70a9bd6bfd1 'E'
  |/
  o  1: dae3b312bb78 'Z'
  |
  o  0: d20a80d4def3 'base'
  
It should also have mutations made on both sides visible.

  $ tglogm -r 'predecessors(all())'
  o  8: 6caded0e9807 'D'
  |
  | o  7: ba83c5428cb2 'F'
  | |
  +---@  6: dd114d9b2f9e 'X'
  | |
  | | o  5: d8fc5ae9b7ef 'D'
  | |/
  | o  4: c70a9bd6bfd1 'E'
  |/
  | x  3: 6ba5de8abe43 'D'  (Rewritten using rebase into 6caded0e9807) (Rewritten using rebase into d8fc5ae9b7ef)
  | |
  | x  2: 2d0f0af04f18 'C'  (Rewritten using amend into c70a9bd6bfd1)
  |/
  o  1: dae3b312bb78 'Z'
  |
  o  0: d20a80d4def3 'base'
  
Introduce a third client that is still using obsmarker-based mutation and visibility
(This is incompatible with narrow-heads)

  $ cd $TESTTMP
  $ hg clone ssh://user@dummy/server client3 -q --config visibility.enabled=false
  migrating repo to old-style visibility and phases
  (this restores the behavior to a known good state; post in Source Control @ FB if you have issues)
  $ cd client3
  $ setconfig commitcloud.servicetype=local commitcloud.servicelocation=$TESTTMP
  $ setconfig commitcloud.user_token_path=$TESTTMP
  $ setconfig mutation.enabled=false
  $ setconfig visibility.enabled=false
  $ setconfig experimental.narrow-heads=false
  $ hg cloud auth -t xxxxxx
  updating authentication token
  authentication successful
  $ hg cloud join
  commitcloud: this repository is now connected to the 'user/test/default' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/default'
  pulling d8fc5ae9b7ef dd114d9b2f9e ba83c5428cb2 6caded0e9807
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 3 files
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 2 files
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 3 files
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 2 files
  commitcloud: commits synchronized
  finished in * sec (glob)
  $ tglogm
  o  6: 6caded0e9807 'D'
  |
  | o  5: ba83c5428cb2 'F'
  | |
  +---o  4: dd114d9b2f9e 'X'
  | |
  | | o  3: d8fc5ae9b7ef 'D'
  | |/
  | o  2: c70a9bd6bfd1 'E'
  |/
  o  1: dae3b312bb78 'Z'
  |
  @  0: d20a80d4def3 'base'
  
  $ cd ../client1
  $ hg up $F
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated to "ba83c5428cb2: F"
  3 other heads for branch "default"
  $ hg amend -m F-amended
  $ hg amend -m F-amended-again
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  backing up stack rooted at dae3b312bb78
  remote: pushing 3 commits:
  remote:     dae3b312bb78  Z
  remote:     c70a9bd6bfd1  E
  remote:     b5ea82a7973c  F-amended-again
  commitcloud: commits synchronized
  finished in * sec (glob)
  $ hg undo
  undone to *, before amend -m F-amended-again (glob)
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  backing up stack rooted at dae3b312bb78
  remote: pushing 3 commits:
  remote:     dae3b312bb78  Z
  remote:     c70a9bd6bfd1  E
  remote:     1ef69cfd595b  F-amended
  commitcloud: commits synchronized
  finished in * sec (glob)
  commitcloud: current revision 1ef69cfd595b has been replaced remotely with multiple revisions
  (run 'hg update HASH' to go to the desired revision)

  $ cd ../client3
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  pulling 1ef69cfd595b
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 3 files
  commitcloud: commits synchronized
  finished in * sec (glob)

  $ cd ../client1
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: commits synchronized
  finished in * sec (glob)
  commitcloud: current revision 1ef69cfd595b has been replaced remotely with multiple revisions
  (run 'hg update HASH' to go to the desired revision)

  $ cd ../client3
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: commits synchronized
  finished in * sec (glob)
  $ tglogm
  o  7: 1ef69cfd595b 'F-amended'
  |
  | o  6: 6caded0e9807 'D'
  | |
  +---o  5: ba83c5428cb2 'F'
  | |
  | | o  4: dd114d9b2f9e 'X'
  | |/
  +---o  3: d8fc5ae9b7ef 'D'
  | |
  o |  2: c70a9bd6bfd1 'E'
  |/
  o  1: dae3b312bb78 'Z'
  |
  @  0: d20a80d4def3 'base'
  
