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
  $ hg commit --amend -l msg

Make sure no invalid manifests were written:

  $ cd .hg/store/packs/manifests
  $ for i in *.histidx; do hg debughistorypack $i; done
  
  
  Node          P1 Node       P2 Node       Link Node     Copy From
  eb7988638387  41b34f08c135  000000000000  220f69710758  
  
  
  Node          P1 Node       P2 Node       Link Node     Copy From
  eb7988638387  41b34f08c135  000000000000  112478962961  
  
  
  Node          P1 Node       P2 Node       Link Node     Copy From
  41b34f08c135  000000000000  000000000000  426bada5c675  
