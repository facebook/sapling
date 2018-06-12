Crash in histpack code path where the amend destination already exists

  $ enable undo inhibit treemanifest fastmanifest remotefilelog
  $ setconfig experimental.evolution=createmarkers treemanifest.treeonly=1 remotefilelog.reponame=foo remotefilelog.cachepath=$TESTTMP/cache
  $ newrepo
  $ drawdag << 'EOS'
  > B
  > |
  > A
  > EOS

  $ enable undo
  $ hg up -q $B
  $ echo foo > msg
  $ hg commit --amend -l msg
  $ hg undo -q
  $ hg commit --amend -l msg 2>&1 | tail -1
  ValueError: attempting to add nullid linknode
