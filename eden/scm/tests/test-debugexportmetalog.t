#chg-compatible

  $ configure modern
  $ setconfig metalog.track-config=0
  $ newrepo
  $ hg commit -m A --config ui.allowemptycommit=1

  $ hg debugexportmetalog exported
  metalog exported to git repo at exported
  use 'git checkout master' to get a working copy
  examples:
    git log -p remotenames     # why remotenames get changed
    git annotate visibleheads  # why a head is added

#if git
  $ cd exported
  $ git log -p -- visibleheads
  commit 509a11fc8f9a1de6296585c539c3bf712a1c2caf
  Author: metalog <metalog@example.com>
  Date:   Thu Jan 1 00:00:00 1970 +0000
  
      commit -m A --config 'ui.allowemptycommit=1'
      Parent: 433fb6a14b4e7044062a8886ddcb13ffa34a78c1
      Transaction: commit
      
      RootId: 31eb05f201b026715d65ecc01175269d9aa69c3e
  
  diff --git a/visibleheads b/visibleheads
  index e69de29..2b0abff 100644
  --- a/visibleheads
  +++ b/visibleheads
  @@ -0,0 +1,2 @@
  +v1
  +7b3f3d5e5faf6c5e4a16fa012fa57ee93d4a6fa1
  
  commit f3993efaf8461778d22d2016019c65383a663e37
  Author: metalog <metalog@example.com>
  Date:   Thu Jan 1 00:00:00 1970 +0000
  
      migrate from vfs
      
      RootId: 433fb6a14b4e7044062a8886ddcb13ffa34a78c1
  
  diff --git a/visibleheads b/visibleheads
  new file mode 100644
  index 0000000..e69de29
#endif
