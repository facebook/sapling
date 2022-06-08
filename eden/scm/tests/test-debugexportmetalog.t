#chg-compatible
#require git

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
  commit ca606bce3d0b617f7faeaefddc49cb515563099c
  Author: metalog <metalog@example.com>
  Date:   Thu Jan 1 00:00:00 1970 +0000
  
      commit -m A --config 'ui.allowemptycommit=1'
      Parent: 22f7ca48c27ae55149b47e140c3f5b9a2bac9e95
      Transaction: commit
      
      RootId: 6ac66be1ccc652ab9dc0ca587305b2cc19fc5d54
  
  diff --git a/visibleheads b/visibleheads
  index e69de29..2b0abff 100644
  --- a/visibleheads
  +++ b/visibleheads
  @@ -0,0 +1,2 @@
  +v1
  +7b3f3d5e5faf6c5e4a16fa012fa57ee93d4a6fa1
  
  commit 09c4e1b6e0621e447ca8399f684aa1f89e1d199d
  Author: metalog <metalog@example.com>
  Date:   Thu Jan 1 00:00:00 1970 +0000
  
      init tracked
      
      RootId: 22f7ca48c27ae55149b47e140c3f5b9a2bac9e95
  
  diff --git a/visibleheads b/visibleheads
  new file mode 100644
  index 0000000..e69de29
