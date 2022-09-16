#chg-compatible

  $ setconfig status.use-rust=False workingcopy.use-rust=False
  $ enable amend rebase undo directaccess shelve
  $ setconfig experimental.evolution=obsolete
  $ setconfig visibility.enabled=true visibility.verbose=true
  $ setconfig mutation.record=true mutation.enabled=true mutation.date="0 0"
  $ setconfig hint.ack=undo

Useful functions
  $ mkcommit()
  > {
  >   echo "$1" > "$1"
  >   hg add "$1"
  >   hg commit -m "$1"
  > }

Setup
  $ newrepo
  $ mkcommit root
  $ mkcommit public1
  $ mkcommit public2
  $ hg debugmakepublic .
  $ hg debugvisibility status
  commit visibility is tracked explicitly

Simple creation and amending of draft commits

  $ mkcommit draft1
  $ hg debugvisibleheads
  ca9d66205acae45570c29bea55877bb8031aa453 draft1
  $ hg amend -m "draft1 amend1"
  $ hg debugvisibleheads
  bc066ca12b451d14668c7a3e38757449b7d6a104 draft1 amend1
  $ mkcommit draft2
  $ tglogp --hidden
  @  467d8aa13aef draft 'draft2'
  │
  o  bc066ca12b45 draft 'draft1 amend1'
  │
  │ x  ca9d66205aca draft 'draft1'
  ├─╯
  o  4f416a252ac8 public 'public2'
  │
  o  175dbab47dcc public 'public1'
  │
  o  1e4be0697311 public 'root'
  
  $ hg debugvisibleheads
  467d8aa13aef105d18160ea682d5cf20d8941d06 draft2

  $ hg debugstrip -r . --config amend.safestrip=False
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ tglogp --hidden
  @  bc066ca12b45 draft 'draft1 amend1'
  │
  │ x  ca9d66205aca draft 'draft1'
  ├─╯
  o  4f416a252ac8 public 'public2'
  │
  o  175dbab47dcc public 'public1'
  │
  o  1e4be0697311 public 'root'
  
  $ hg debugvisibleheads
  bc066ca12b451d14668c7a3e38757449b7d6a104 draft1 amend1

# Quick test of children revsets when there is a hidden child.
  $ hg log -r 'desc("public2")~-1' -T '{desc}\n'
  draft1 amend1
  $ hg log -r 'children(desc("public2"))' -T '{desc}\n'
  draft1 amend1


  $ mkcommit draft2a
  $ hg rebase -s ".^" -d 'desc(public1)'
  rebasing bc066ca12b45 "draft1 amend1"
  rebasing 2ccd7cddaa94 "draft2a"
  $ tglogp
  @  ecfc0c412bb8 draft 'draft2a'
  │
  o  96b7359a7ee5 draft 'draft1 amend1'
  │
  │ o  4f416a252ac8 public 'public2'
  ├─╯
  o  175dbab47dcc public 'public1'
  │
  o  1e4be0697311 public 'root'
  
  $ hg debugvisibleheads
  ecfc0c412bb878c3e7b1b3468cae773b473fd3ec draft2a
  $ hg rebase -s . -d 'desc(public2)'
  rebasing ecfc0c412bb8 "draft2a"
  $ tglogp
  @  af54c09bb37d draft 'draft2a'
  │
  │ o  96b7359a7ee5 draft 'draft1 amend1'
  │ │
  o │  4f416a252ac8 public 'public2'
  ├─╯
  o  175dbab47dcc public 'public1'
  │
  o  1e4be0697311 public 'root'
  
  $ hg debugvisibleheads
  af54c09bb37da36975b8d482f660f62f95697a35 draft2a
  96b7359a7ee5350b94be6e5c5dd480751a031498 draft1 amend1

Add more commits

  $ mkcommit draft3
  $ mkcommit draft4
  $ hg merge -q 'max(desc(draft1))'
  $ hg commit -m "merge1"
  $ hg up -q 'max(desc(draft1))'
  $ hg merge -q 'desc(draft4)'
  $ hg commit -m "merge2"
  $ hg debugmakepublic 'desc(root)'

  $ tglogp
  @    8a541e4b5b52 draft 'merge2'
  ├─╮
  │ │ o  00c8b0f0741e draft 'merge1'
  ╭─┬─╯
  │ o  f3f5679a1c9c draft 'draft4'
  │ │
  │ o  5dabc7b08ef9 draft 'draft3'
  │ │
  │ o  af54c09bb37d draft 'draft2a'
  │ │
  o │  96b7359a7ee5 draft 'draft1 amend1'
  │ │
  │ o  4f416a252ac8 draft 'public2'
  ├─╯
  o  175dbab47dcc draft 'public1'
  │
  o  1e4be0697311 public 'root'
  
Hide and unhide

  $ hg up -q 'desc(root)'
  $ hg hide 'desc(merge1)'
  hiding commit 00c8b0f0741e "merge1"
  1 changeset hidden
  $ hg debugvisibleheads
  8a541e4b5b528ca9db5d1f8afd4f2534fcd79527 merge2
  $ hg hide 'max(desc(draft2a))'
  hiding commit af54c09bb37d "draft2a"
  hiding commit 5dabc7b08ef9 "draft3"
  hiding commit f3f5679a1c9c "draft4"
  hiding commit 8a541e4b5b52 "merge2"
  4 changesets hidden
  $ hg debugvisibleheads
  96b7359a7ee5350b94be6e5c5dd480751a031498 draft1 amend1
  4f416a252ac81004d9b35542cb1dc8892b6879eb public2
  $ hg unhide 'desc(draft3)'
  $ hg debugvisibleheads
  96b7359a7ee5350b94be6e5c5dd480751a031498 draft1 amend1
  5dabc7b08ef934b9e6720285205b2c17695f6491 draft3
  $ hg hide 'desc(public2)' 'desc(amend1)'
  hiding commit 4f416a252ac8 "public2"
  hiding commit 96b7359a7ee5 "draft1 amend1"
  hiding commit af54c09bb37d "draft2a"
  hiding commit 5dabc7b08ef9 "draft3"
  4 changesets hidden
  $ hg debugvisibleheads
  175dbab47dccefd3ece5916c4f92a6c69f65fcf0 public1
  $ hg unhide 'max(desc(draft1))'
  $ hg debugvisibleheads
  96b7359a7ee5350b94be6e5c5dd480751a031498 draft1 amend1
  $ hg hide 'desc(public1)'
  hiding commit 175dbab47dcc "public1"
  hiding commit 96b7359a7ee5 "draft1 amend1"
  2 changesets hidden
  $ hg debugvisibleheads
  $ hg unhide 'desc(merge1)'
  $ hg debugvisibleheads
  00c8b0f0741e6ef0696abd63aba22f3d49018b38 merge1
  $ hg unhide 'desc(merge2)'
  $ hg debugvisibleheads
  00c8b0f0741e6ef0696abd63aba22f3d49018b38 merge1
  8a541e4b5b528ca9db5d1f8afd4f2534fcd79527 merge2

Stack navigation and rebases

  $ newrepo
  $ drawdag << EOS
  > E
  > |
  > D
  > |
  > C
  > |
  > B
  > |
  > A
  > EOS
  $ hg up $B
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg amend -m "B amended" --no-rebase
  hint[amend-restack]: descendants of 112478962961 are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints
  $ tglogm
  @  e60094faeb72 'B amended'
  │
  │ o  9bc730a19041 'E'
  │ │
  │ o  f585351a92f8 'D'
  │ │
  │ o  26805aba1e60 'C'
  │ │
  │ x  112478962961 'B'  (Rewritten using amend into e60094faeb72)
  ├─╯
  o  426bada5c675 'A'
  
  $ hg next --rebase
  rebasing 26805aba1e60 "C"
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [23910a] C
  $ tglogm
  @  23910a6fe564 'C'
  │
  o  e60094faeb72 'B amended'
  │
  │ o  9bc730a19041 'E'
  │ │
  │ o  f585351a92f8 'D'
  │ │
  │ x  26805aba1e60 'C'  (Rewritten using rebase into 23910a6fe564)
  │ │
  │ x  112478962961 'B'  (Rewritten using amend into e60094faeb72)
  ├─╯
  o  426bada5c675 'A'
  
  $ hg next --rebase
  rebasing f585351a92f8 "D"
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [1d30cc] D
  $ tglogm
  @  1d30cc995ea7 'D'
  │
  o  23910a6fe564 'C'
  │
  o  e60094faeb72 'B amended'
  │
  │ o  9bc730a19041 'E'
  │ │
  │ x  f585351a92f8 'D'  (Rewritten using rebase into 1d30cc995ea7)
  │ │
  │ x  26805aba1e60 'C'  (Rewritten using rebase into 23910a6fe564)
  │ │
  │ x  112478962961 'B'  (Rewritten using amend into e60094faeb72)
  ├─╯
  o  426bada5c675 'A'
  
  $ hg next --rebase
  rebasing 9bc730a19041 "E"
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [ec992f] E
  $ tglogm
  @  ec992ff1fd78 'E'
  │
  o  1d30cc995ea7 'D'
  │
  o  23910a6fe564 'C'
  │
  o  e60094faeb72 'B amended'
  │
  o  426bada5c675 'A'
  

Undo

  $ hg undo
  undone to *, before next --rebase (glob)
  $ tglogm
  @  1d30cc995ea7 'D'
  │
  o  23910a6fe564 'C'
  │
  o  e60094faeb72 'B amended'
  │
  │ o  9bc730a19041 'E'
  │ │
  │ x  f585351a92f8 'D'  (Rewritten using rebase into 1d30cc995ea7)
  │ │
  │ x  26805aba1e60 'C'  (Rewritten using rebase into 23910a6fe564)
  │ │
  │ x  112478962961 'B'  (Rewritten using amend into e60094faeb72)
  ├─╯
  o  426bada5c675 'A'
  
  $ hg undo
  undone to *, before next --rebase (glob)
  $ tglogm
  @  23910a6fe564 'C'
  │
  o  e60094faeb72 'B amended'
  │
  │ o  9bc730a19041 'E'
  │ │
  │ o  f585351a92f8 'D'
  │ │
  │ x  26805aba1e60 'C'  (Rewritten using rebase into 23910a6fe564)
  │ │
  │ x  112478962961 'B'  (Rewritten using amend into e60094faeb72)
  ├─╯
  o  426bada5c675 'A'
  
  $ hg undo
  undone to *, before next --rebase (glob)
  $ tglogm
  @  e60094faeb72 'B amended'
  │
  │ o  9bc730a19041 'E'
  │ │
  │ o  f585351a92f8 'D'
  │ │
  │ o  26805aba1e60 'C'
  │ │
  │ x  112478962961 'B'  (Rewritten using amend into e60094faeb72)
  ├─╯
  o  426bada5c675 'A'
  
Also check the obsolete revset is consistent.
  $ tglogm -r "obsolete()"
  x  112478962961 'B'  (Rewritten using amend into e60094faeb72)
  │
  ~
  $ tglogm --hidden -r "obsolete()"
  x  9bc730a19041 'E'  (Rewritten using rebase into ec992ff1fd78)
  │
  x  f585351a92f8 'D'  (Rewritten using rebase into 1d30cc995ea7)
  │
  x  26805aba1e60 'C'  (Rewritten using rebase into 23910a6fe564)
  │
  x  112478962961 'B'  (Rewritten using amend into e60094faeb72)
  │
  ~

Unhiding them reveals them as new commits and now the old ones show their relationship
to the new ones.
  $ hg unhide ec992ff1fd78
  $ tglogm
  o  ec992ff1fd78 'E'
  │
  o  1d30cc995ea7 'D'
  │
  o  23910a6fe564 'C'
  │
  @  e60094faeb72 'B amended'
  │
  │ x  9bc730a19041 'E'  (Rewritten using rebase into ec992ff1fd78)
  │ │
  │ x  f585351a92f8 'D'  (Rewritten using rebase into 1d30cc995ea7)
  │ │
  │ x  26805aba1e60 'C'  (Rewritten using rebase into 23910a6fe564)
  │ │
  │ x  112478962961 'B'  (Rewritten using amend into e60094faeb72)
  ├─╯
  o  426bada5c675 'A'
  
Test that hiddenoverride has no effect on pinning hidden revisions.
  $ cd $TESTTMP
  $ newrepo
  $ drawdag << EOS
  > B D F
  > | | |
  > A C E  # amend: A -> C -> E
  >  \|/   # rebase: B -> D -> F
  >   Z
  > EOS
  $ tglogm
  o  a77c932a84af 'F'
  │
  o  05eb30556340 'E'
  │
  o  48b9aae0607f 'Z'
  
  $ hg up -q 917a077edb8d # Update to B
  $ tglogm
  o  a77c932a84af 'F'
  │
  o  05eb30556340 'E'
  │
  │ @  917a077edb8d 'B'  (Rewritten using rewrite into a77c932a84af)
  │ │
  │ x  ac2f7407182b 'A'  (Rewritten using rewrite into 05eb30556340)
  ├─╯
  o  48b9aae0607f 'Z'
  
  $ hg up -q $F
  $ tglogm
  @  a77c932a84af 'F'
  │
  o  05eb30556340 'E'
  │
  o  48b9aae0607f 'Z'
  
Test that shelve and unshelve work
  $ echo more > file
  $ hg add file
  $ hg st
  A file
  $ hg shelve
  shelved as default
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg st
  $ tglogm
  @  a77c932a84af 'F'
  │
  o  05eb30556340 'E'
  │
  o  48b9aae0607f 'Z'
  
  $ hg prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [05eb30] E
  $ hg unshelve --keep
  unshelving change 'default'
  rebasing shelved changes
  rebasing f321a4a9343c "shelve changes to: F"
  $ hg st
  A file
  $ tglogm
  o  a77c932a84af 'F'
  │
  @  05eb30556340 'E'
  │
  o  48b9aae0607f 'Z'
  
  $ hg prev --clean
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [48b9aa] Z
  $ echo data > other
  $ hg add other
  $ hg st
  A other
  ? file
  $ hg unshelve
  unshelving change 'default'
  temporarily committing pending changes (restore with 'hg unshelve --abort')
  rebasing shelved changes
  rebasing f321a4a9343c "shelve changes to: F"
  $ hg st
  A file
  A other
  $ tglogm
  o  a77c932a84af 'F'
  │
  o  05eb30556340 'E'
  │
  @  48b9aae0607f 'Z'
  
Test undo of split
  $ cd $TESTTMP
  $ newrepo
  $ echo base > base
  $ hg commit -Aqm base
  $ echo file1 > file1
  $ echo file2 > file2
  $ echo file3 > file3
  $ hg commit -Aqm to-split
  $ hg split --config ui.interactive=true << EOF
  > y
  > y
  > n
  > n
  > n
  > y
  > y
  > n
  > y
  > EOF
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved
  adding file1
  adding file2
  adding file3
  diff --git a/file1 b/file1
  new file mode 100644
  examine changes to 'file1'? [Ynesfdaq?] y
  
  @@ -0,0 +1,1 @@
  +file1
  record change 1/3 to 'file1'? [Ynesfdaq?] y
  
  diff --git a/file2 b/file2
  new file mode 100644
  examine changes to 'file2'? [Ynesfdaq?] n
  
  diff --git a/file3 b/file3
  new file mode 100644
  examine changes to 'file3'? [Ynesfdaq?] n
  
  Done splitting? [yN] n
  diff --git a/file2 b/file2
  new file mode 100644
  examine changes to 'file2'? [Ynesfdaq?] y
  
  @@ -0,0 +1,1 @@
  +file2
  record change 1/2 to 'file2'? [Ynesfdaq?] y
  
  diff --git a/file3 b/file3
  new file mode 100644
  examine changes to 'file3'? [Ynesfdaq?] n
  
  Done splitting? [yN] y
  $ tglogm
  @  a30320c497f0 'to-split'
  │
  o  0a2500cbe503 'to-split'
  │
  o  06e40e6ae08c 'to-split'
  │
  o  d20a80d4def3 'base'
  
  $ hg undo
  undone to *, before split --config ui.interactive=true (glob)
  $ tglogm
  @  9a8c420e44f2 'to-split'
  │
  o  d20a80d4def3 'base'
  
Unamend and Uncommit
  $ cd $TESTTMP
  $ newrepo
  $ touch base
  $ hg commit -Aqm base
  $ echo 1 > file
  $ hg commit -Aqm commit1
  $ echo 2 > file
  $ hg amend -m commit2
  $ tglogm --hidden
  @  8e8ec65c0bb7 'commit2'
  │
  │ x  4c5b9b3e14b9 'commit1'  (Rewritten using amend into 8e8ec65c0bb7)
  ├─╯
  o  df4f53cec30a 'base'
  

  $ hg unamend
  $ tglogm
  @  4c5b9b3e14b9 'commit1'
  │
  o  df4f53cec30a 'base'
  
  $ tglogm --hidden
  o  8e8ec65c0bb7 'commit2'
  │
  │ @  4c5b9b3e14b9 'commit1'  (Rewritten using amend into 8e8ec65c0bb7)
  ├─╯
  o  df4f53cec30a 'base'
  

  $ hg uncommit
  $ tglogm
  @  df4f53cec30a 'base'
  
  $ tglogm --hidden
  o  8e8ec65c0bb7 'commit2'
  │
  │ x  4c5b9b3e14b9 'commit1'  (Rewritten using amend into 8e8ec65c0bb7)
  ├─╯
  @  df4f53cec30a 'base'
  

Hidden revset
  $ hg log --graph -r 'hidden()'
  o  commit:      8e8ec65c0bb7
  │  user:        test
  ~  date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     commit2
  
  o  commit:      4c5b9b3e14b9
  │  user:        test
  ~  date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     commit1
  

Migration down
  $ setconfig visibility.enabled=false
  $ hg debugedenimporthelper --get-manifest-node df4f53cec30af1e4f669102135076fd4f9673fcc
  reverting to tracking visibility through obsmarkers
  4e7eb8574ed56675aa89d2b5abbced12d5688cef

Migration up
  $ setconfig visibility.enabled=true

 (Test if the repo contains an abandoned transaction, the auto migration does not crash)
  $ echo something > .hg/store/journal
  $ hg debugedenimporthelper --get-manifest-node df4f53cec30af1e4f669102135076fd4f9673fcc
  switching to explicit tracking of visible commits
  4e7eb8574ed56675aa89d2b5abbced12d5688cef
  $ rm .hg/store/journal

  $ hg debugedenimporthelper --get-manifest-node df4f53cec30af1e4f669102135076fd4f9673fcc
  switching to explicit tracking of visible commits
  4e7eb8574ed56675aa89d2b5abbced12d5688cef
