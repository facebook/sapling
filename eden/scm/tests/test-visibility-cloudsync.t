#chg-compatible
#debugruntest-compatible
  $ setconfig experimental.allowfilepeer=True

  $ configure dummyssh
  $ enable amend directaccess commitcloud infinitepush rebase remotenames undo
  $ setconfig infinitepush.branchpattern="re:scratch/.*"
  $ setconfig commitcloud.hostname=testhost
  $ setconfig visibility.enabled=true
  $ setconfig experimental.evolution=obsolete
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
  $ setconfig commitcloud.servicetype=local commitcloud.servicelocation=$TESTTMP commitcloud.token_enforced=False
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
  o  c70a9bd6bfd1 'E'
  │
  │ o  6ba5de8abe43 'D'
  │ │
  │ x  2d0f0af04f18 'C'  (Rewritten using amend into c70a9bd6bfd1)
  ├─╯
  o  dae3b312bb78 'Z'
  │
  @  d20a80d4def3 'base'
  
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  backing up stack rooted at dae3b312bb78
  commitcloud: commits synchronized
  finished in * sec (glob)
  remote: pushing 4 commits:
  remote:     dae3b312bb78  Z
  remote:     2d0f0af04f18  C
  remote:     6ba5de8abe43  D
  remote:     c70a9bd6bfd1  E

Create another client and use it to modify the commits and create some new ones.

  $ cd $TESTTMP
  $ hg clone ssh://user@dummy/server client2 -q
  $ cd client2
  $ setconfig commitcloud.servicetype=local commitcloud.servicelocation=$TESTTMP commitcloud.token_enforced=False
  $ hg cloud join
  commitcloud: this repository is now connected to the 'user/test/default' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/default'
  pulling 6ba5de8abe43 c70a9bd6bfd1 from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  adding changesets
  adding manifests
  adding file changes
  commitcloud: commits synchronized
  finished in * sec (glob)
  $ tglogm
  o  c70a9bd6bfd1 'E'
  │
  │ o  6ba5de8abe43 'D'
  │ │
  │ x  2d0f0af04f18 'C'  (Rewritten using amend into c70a9bd6bfd1)
  ├─╯
  o  dae3b312bb78 'Z'
  │
  @  d20a80d4def3 'base'
  

  $ hg rebase -r $D -d $E
  rebasing 6ba5de8abe43 "D"
  $ hg up -q $Z
  $ echo X > X
  $ hg commit -Aqm X
  $ tglogm
  @  dd114d9b2f9e 'X'
  │
  │ o  d8fc5ae9b7ef 'D'
  │ │
  │ o  c70a9bd6bfd1 'E'
  ├─╯
  o  dae3b312bb78 'Z'
  │
  o  d20a80d4def3 'base'
  
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  backing up stack rooted at dae3b312bb78
  commitcloud: commits synchronized
  finished in * sec (glob)
  remote: pushing 4 commits:
  remote:     dae3b312bb78  Z
  remote:     c70a9bd6bfd1  E
  remote:     d8fc5ae9b7ef  D
  remote:     dd114d9b2f9e  X

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
  o  6caded0e9807 'D'
  │
  │ @  ba83c5428cb2 'F'
  │ │
  │ o  c70a9bd6bfd1 'E'
  ├─╯
  o  dae3b312bb78 'Z'
  │
  o  d20a80d4def3 'base'
  
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  backing up stack rooted at dae3b312bb78
  pulling d8fc5ae9b7ef dd114d9b2f9e from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  adding changesets
  adding manifests
  adding file changes
  remote: pushing 4 commits:
  remote:     dae3b312bb78  Z
  remote:     c70a9bd6bfd1  E
  remote:     ba83c5428cb2  F
  remote:     6caded0e9807  D
  commitcloud: commits synchronized
  finished in * sec (glob)
  $ tglogm
  o  dd114d9b2f9e 'X'
  │
  │ o  d8fc5ae9b7ef 'D'
  │ │
  │ │ o  6caded0e9807 'D'
  ├───╯
  │ │ @  ba83c5428cb2 'F'
  │ ├─╯
  │ o  c70a9bd6bfd1 'E'
  ├─╯
  o  dae3b312bb78 'Z'
  │
  o  d20a80d4def3 'base'
  

Cloud sync back to the other client, it should get the same smartlog (apart from ordering).

  $ cd $TESTTMP/client2
  $ hg cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  pulling ba83c5428cb2 6caded0e9807 from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  adding changesets
  adding manifests
  adding file changes
  commitcloud: commits synchronized
  finished in * sec (glob)
  $ tglogm
  o  ba83c5428cb2 'F'
  │
  │ o  6caded0e9807 'D'
  │ │
  │ │ @  dd114d9b2f9e 'X'
  │ ├─╯
  │ │ o  d8fc5ae9b7ef 'D'
  ├───╯
  o │  c70a9bd6bfd1 'E'
  ├─╯
  o  dae3b312bb78 'Z'
  │
  o  d20a80d4def3 'base'
  
It should also have mutations made on both sides visible.

  $ tglogm -r 'predecessors(all())'
  o  ba83c5428cb2 'F'
  │
  │ o  6caded0e9807 'D'
  │ │
  │ │ @  dd114d9b2f9e 'X'
  │ ├─╯
  │ │ o  d8fc5ae9b7ef 'D'
  ├───╯
  o │  c70a9bd6bfd1 'E'
  ├─╯
  │ x  6ba5de8abe43 'D'  (Rewritten using rebase into 6caded0e9807) (Rewritten using rebase into d8fc5ae9b7ef)
  │ │
  │ x  2d0f0af04f18 'C'  (Rewritten using amend into c70a9bd6bfd1)
  ├─╯
  o  dae3b312bb78 'Z'
  │
  o  d20a80d4def3 'base'
  
