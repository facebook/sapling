#debugruntest-compatible

#require no-eden


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
  $ configure modernclient
  $ newclientrepo
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
  5b93956a25ec5ed476b39b46bbdd1efdfdf0ee6a draft1 amend1
  $ mkcommit draft2
  $ tglogp --hidden
  @  7ff6bbfeb971 draft 'draft2'
  │
  o  5b93956a25ec draft 'draft1 amend1'
  │
  │ x  ca9d66205aca draft 'draft1'
  ├─╯
  o  4f416a252ac8 public 'public2'
  │
  o  175dbab47dcc public 'public1'
  │
  o  1e4be0697311 public 'root'
  
  $ hg debugvisibleheads
  7ff6bbfeb971e769d9f5821bcdd4f42b446603d6 draft2

  $ hg debugstrip -r . --config amend.safestrip=False
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ tglogp --hidden
  @  5b93956a25ec draft 'draft1 amend1'
  │
  │ x  ca9d66205aca draft 'draft1'
  ├─╯
  o  4f416a252ac8 public 'public2'
  │
  o  175dbab47dcc public 'public1'
  │
  o  1e4be0697311 public 'root'
  
  $ hg debugvisibleheads
  5b93956a25ec5ed476b39b46bbdd1efdfdf0ee6a draft1 amend1

# Quick test of children revsets when there is a hidden child.
  $ hg log -r 'desc("public2")~-1' -T '{desc}\n'
  draft1 amend1
  $ hg log -r 'children(desc("public2"))' -T '{desc}\n'
  draft1 amend1


  $ mkcommit draft2a
  $ hg rebase -s ".^" -d 'desc(public1)'
  rebasing 5b93956a25ec "draft1 amend1"
  rebasing 51e3d0f3d402 "draft2a"
  $ tglogp
  @  6c98fad065b9 draft 'draft2a'
  │
  o  e500bf5639fd draft 'draft1 amend1'
  │
  │ o  4f416a252ac8 public 'public2'
  ├─╯
  o  175dbab47dcc public 'public1'
  │
  o  1e4be0697311 public 'root'
  
  $ hg debugvisibleheads
  6c98fad065b92f276ba38ad20df31312c0bcf342 draft2a
  $ hg rebase -s . -d 'desc(public2)'
  rebasing 6c98fad065b9 "draft2a"
  $ tglogp
  @  5b3624aa14e4 draft 'draft2a'
  │
  │ o  e500bf5639fd draft 'draft1 amend1'
  │ │
  o │  4f416a252ac8 public 'public2'
  ├─╯
  o  175dbab47dcc public 'public1'
  │
  o  1e4be0697311 public 'root'
  
  $ hg debugvisibleheads
  5b3624aa14e4e48b969670853d0a0eb95dfaa13a draft2a
  e500bf5639fd8c869a8cf4e10b651aaa01bfa42c draft1 amend1

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
  @    4daf58216052 draft 'merge2'
  ├─╮
  │ │ o  6abdecc48cd1 draft 'merge1'
  ╭─┬─╯
  │ o  02227f0b851e draft 'draft4'
  │ │
  │ o  4975507bccc9 draft 'draft3'
  │ │
  │ o  5b3624aa14e4 draft 'draft2a'
  │ │
  o │  e500bf5639fd draft 'draft1 amend1'
  │ │
  │ o  4f416a252ac8 draft 'public2'
  ├─╯
  o  175dbab47dcc draft 'public1'
  │
  o  1e4be0697311 public 'root'
  
Hide and unhide

  $ hg up -q 'desc(root)'
  $ hg hide 'desc(merge1)'
  hiding commit 6abdecc48cd1 "merge1"
  1 changeset hidden
  $ hg debugvisibleheads
  4daf58216052b220cae410be4e8607c11f7ad7c2 merge2
  $ hg hide 'max(desc(draft2a))'
  hiding commit 5b3624aa14e4 "draft2a"
  hiding commit 4975507bccc9 "draft3"
  hiding commit 02227f0b851e "draft4"
  hiding commit 4daf58216052 "merge2"
  4 changesets hidden
  $ hg debugvisibleheads
  e500bf5639fd8c869a8cf4e10b651aaa01bfa42c draft1 amend1
  4f416a252ac81004d9b35542cb1dc8892b6879eb public2
  $ hg unhide 'desc(draft3)'
  $ hg debugvisibleheads
  e500bf5639fd8c869a8cf4e10b651aaa01bfa42c draft1 amend1
  4975507bccc988c9fe1822b1ec33e754ae0b0334 draft3
  $ hg hide 'desc(public2)' 'desc(amend1)'
  hiding commit 4f416a252ac8 "public2"
  hiding commit e500bf5639fd "draft1 amend1"
  hiding commit 5b3624aa14e4 "draft2a"
  hiding commit 4975507bccc9 "draft3"
  4 changesets hidden
  $ hg debugvisibleheads
  175dbab47dccefd3ece5916c4f92a6c69f65fcf0 public1
  $ hg unhide 'max(desc(draft1))'
  $ hg debugvisibleheads
  e500bf5639fd8c869a8cf4e10b651aaa01bfa42c draft1 amend1
  $ hg hide 'desc(public1)'
  hiding commit 175dbab47dcc "public1"
  hiding commit e500bf5639fd "draft1 amend1"
  2 changesets hidden
  $ hg debugvisibleheads
  $ hg unhide 'desc(merge1)'
  $ hg debugvisibleheads
  6abdecc48cd1aa6c444bb3abed96fee7af294684 merge1
  $ hg unhide 'desc(merge2)'
  $ hg debugvisibleheads
  6abdecc48cd1aa6c444bb3abed96fee7af294684 merge1
  4daf58216052b220cae410be4e8607c11f7ad7c2 merge2

Stack navigation and rebases

  $ newclientrepo
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
  @  480c8b61ccd9 'B amended'
  │
  │ o  9bc730a19041 'E'
  │ │
  │ o  f585351a92f8 'D'
  │ │
  │ o  26805aba1e60 'C'
  │ │
  │ x  112478962961 'B'  (Rewritten using amend into 480c8b61ccd9)
  ├─╯
  o  426bada5c675 'A'
  
  $ hg next --rebase
  rebasing 26805aba1e60 "C"
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [a0452d] C
  $ tglogm
  @  a0452d36adc9 'C'
  │
  o  480c8b61ccd9 'B amended'
  │
  │ o  9bc730a19041 'E'
  │ │
  │ o  f585351a92f8 'D'
  │ │
  │ x  26805aba1e60 'C'  (Rewritten using rebase into a0452d36adc9)
  │ │
  │ x  112478962961 'B'  (Rewritten using amend into 480c8b61ccd9)
  ├─╯
  o  426bada5c675 'A'
  
  $ hg next --rebase
  rebasing f585351a92f8 "D"
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [a00f83] D
  $ tglogm
  @  a00f837dfca6 'D'
  │
  o  a0452d36adc9 'C'
  │
  o  480c8b61ccd9 'B amended'
  │
  │ o  9bc730a19041 'E'
  │ │
  │ x  f585351a92f8 'D'  (Rewritten using rebase into a00f837dfca6)
  │ │
  │ x  26805aba1e60 'C'  (Rewritten using rebase into a0452d36adc9)
  │ │
  │ x  112478962961 'B'  (Rewritten using amend into 480c8b61ccd9)
  ├─╯
  o  426bada5c675 'A'
  
  $ hg next --rebase
  rebasing 9bc730a19041 "E"
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [ef1bf9] E
  $ tglogm
  @  ef1bf99db56b 'E'
  │
  o  a00f837dfca6 'D'
  │
  o  a0452d36adc9 'C'
  │
  o  480c8b61ccd9 'B amended'
  │
  o  426bada5c675 'A'
  

Undo

  $ hg undo
  undone to *, before next --rebase (glob)
  $ tglogm
  @  a00f837dfca6 'D'
  │
  o  a0452d36adc9 'C'
  │
  o  480c8b61ccd9 'B amended'
  │
  │ o  9bc730a19041 'E'
  │ │
  │ x  f585351a92f8 'D'  (Rewritten using rebase into a00f837dfca6)
  │ │
  │ x  26805aba1e60 'C'  (Rewritten using rebase into a0452d36adc9)
  │ │
  │ x  112478962961 'B'  (Rewritten using amend into 480c8b61ccd9)
  ├─╯
  o  426bada5c675 'A'
  
  $ hg undo
  undone to *, before next --rebase (glob)
  $ tglogm
  @  a0452d36adc9 'C'
  │
  o  480c8b61ccd9 'B amended'
  │
  │ o  9bc730a19041 'E'
  │ │
  │ o  f585351a92f8 'D'
  │ │
  │ x  26805aba1e60 'C'  (Rewritten using rebase into a0452d36adc9)
  │ │
  │ x  112478962961 'B'  (Rewritten using amend into 480c8b61ccd9)
  ├─╯
  o  426bada5c675 'A'
  
  $ hg undo
  undone to *, before next --rebase (glob)
  $ tglogm
  @  480c8b61ccd9 'B amended'
  │
  │ o  9bc730a19041 'E'
  │ │
  │ o  f585351a92f8 'D'
  │ │
  │ o  26805aba1e60 'C'
  │ │
  │ x  112478962961 'B'  (Rewritten using amend into 480c8b61ccd9)
  ├─╯
  o  426bada5c675 'A'
  
Also check the obsolete revset is consistent.
  $ tglogm -r "obsolete()"
  x  112478962961 'B'  (Rewritten using amend into 480c8b61ccd9)
  │
  ~
  $ tglogm --hidden -r "obsolete()"
  x  9bc730a19041 'E'  (Rewritten using rebase into ef1bf99db56b)
  │
  x  f585351a92f8 'D'  (Rewritten using rebase into a00f837dfca6)
  │
  x  26805aba1e60 'C'  (Rewritten using rebase into a0452d36adc9)
  │
  x  112478962961 'B'  (Rewritten using amend into 480c8b61ccd9)
  │
  ~

Unhiding them reveals them as new commits and now the old ones show their relationship
to the new ones.
  $ hg unhide ef1bf99db56b
  $ tglogm
  o  ef1bf99db56b 'E'
  │
  o  a00f837dfca6 'D'
  │
  o  a0452d36adc9 'C'
  │
  @  480c8b61ccd9 'B amended'
  │
  │ x  9bc730a19041 'E'  (Rewritten using rebase into ef1bf99db56b)
  │ │
  │ x  f585351a92f8 'D'  (Rewritten using rebase into a00f837dfca6)
  │ │
  │ x  26805aba1e60 'C'  (Rewritten using rebase into a0452d36adc9)
  │ │
  │ x  112478962961 'B'  (Rewritten using amend into 480c8b61ccd9)
  ├─╯
  o  426bada5c675 'A'
  
Test that hiddenoverride has no effect on pinning hidden revisions.
  $ cd $TESTTMP
  $ newclientrepo
  $ drawdag << EOS
  > B D F
  > | | |
  > A C E  # amend: A -> C -> E
  >  \|/   # rebase: B -> D -> F
  >   Z
  > EOS
  $ tglogm
  o  12f43da6ed39 'F'
  │
  o  ec4d05032fe4 'E'
  │
  o  48b9aae0607f 'Z'
  
  $ hg up -q 917a077edb8d # Update to B
  $ tglogm
  o  12f43da6ed39 'F'
  │
  o  ec4d05032fe4 'E'
  │
  │ @  917a077edb8d 'B'  (Rewritten using rewrite into 12f43da6ed39)
  │ │
  │ x  ac2f7407182b 'A'  (Rewritten using rewrite into ec4d05032fe4)
  ├─╯
  o  48b9aae0607f 'Z'
  
  $ hg up -q $F
  $ tglogm
  @  12f43da6ed39 'F'
  │
  o  ec4d05032fe4 'E'
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
  @  12f43da6ed39 'F'
  │
  o  ec4d05032fe4 'E'
  │
  o  48b9aae0607f 'Z'
  
  $ hg prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [ec4d05] E
  $ hg unshelve --keep
  unshelving change 'default'
  rebasing shelved changes
  rebasing 43c5c8656322 "shelve changes to: F"
  $ hg st
  A file
  $ tglogm
  o  12f43da6ed39 'F'
  │
  @  ec4d05032fe4 'E'
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
  rebasing 43c5c8656322 "shelve changes to: F"
  $ hg st
  A file
  A other
  $ tglogm
  o  12f43da6ed39 'F'
  │
  o  ec4d05032fe4 'E'
  │
  @  48b9aae0607f 'Z'
  
Test undo of split
  $ cd $TESTTMP
  $ newclientrepo
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
  @  fbb9c1282bfb 'to-split'
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
  $ newclientrepo
  $ touch base
  $ hg commit -Aqm base
  $ echo 1 > file
  $ hg commit -Aqm commit1
  $ echo 2 > file
  $ hg amend -m commit2
  $ tglogm --hidden
  @  e70c2acd5a58 'commit2'
  │
  │ x  4c5b9b3e14b9 'commit1'  (Rewritten using amend into e70c2acd5a58)
  ├─╯
  o  df4f53cec30a 'base'
  

  $ hg unamend
  $ tglogm
  @  4c5b9b3e14b9 'commit1'
  │
  o  df4f53cec30a 'base'
  
  $ tglogm --hidden
  o  e70c2acd5a58 'commit2'
  │
  │ @  4c5b9b3e14b9 'commit1'  (Rewritten using amend into e70c2acd5a58)
  ├─╯
  o  df4f53cec30a 'base'
  

  $ hg uncommit
  $ tglogm
  @  df4f53cec30a 'base'
  
  $ tglogm --hidden
  o  e70c2acd5a58 'commit2'
  │
  │ x  4c5b9b3e14b9 'commit1'  (Rewritten using amend into e70c2acd5a58)
  ├─╯
  @  df4f53cec30a 'base'
  

Hidden revset
  $ hg log --graph -r 'hidden()'
  o  commit:      e70c2acd5a58
  │  user:        test
  ~  date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     commit2
  
  o  commit:      4c5b9b3e14b9
  │  user:        test
  ~  date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     commit1
  
