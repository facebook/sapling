#chg-compatible
#require git
#debugruntest-compatible

  $ configure modern
  $ setconfig metalog.track-config=0
  $ newrepo
  $ hg commit -m A --config ui.allowemptycommit=1

  $ hg debugexportmetalog exported
  metalog exported to git repo at exported
  use 'git checkout main' to get a working copy
  examples:
    git log -p remotenames     # why remotenames get changed
    git annotate visibleheads  # why a head is added

  $ cd exported
  $ git log -p -- visibleheads
  commit * (glob)
  Author: metalog <metalog@example.com>
  Date:   Thu Jan 1 00:00:00 1970 +0000
  
      commit -m A --config 'ui.allowemptycommit=1'
      Parent: e0c47396402d4bbc0eb4f8672ada4951ebc09dc6
      Transaction: commit
      
      RootId: * (glob)
  
  diff --git a/visibleheads b/visibleheads
  index e69de29..2b0abff 100644
  --- a/visibleheads
  +++ b/visibleheads
  @@ -0,0 +1,2 @@
  +v1
  +7b3f3d5e5faf6c5e4a16fa012fa57ee93d4a6fa1
  
  commit * (glob)
  Author: metalog <metalog@example.com>
  Date:   Thu Jan 1 00:00:00 1970 +0000
  
      init tracked
      
      RootId: * (glob)
  
  diff --git a/visibleheads b/visibleheads
  new file mode 100644
  index 0000000..e69de29
