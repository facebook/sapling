
#require no-eden


  $ enable amend rebase undo directaccess shelve
  $ setconfig experimental.evolution=obsolete
  $ setconfig visibility.enabled=true visibility.verbose=true
  $ setconfig mutation.enabled=true
  $ setconfig checkout.obsolete-mode=ignore
  $ setconfig commit.modify-obsolete-mode=ignore
  $ setconfig hint.ack=undo

Useful functions
  $ mkcommit()
  > {
  >   echo "$1" > "$1"
  >   sl add "$1"
  >   sl commit -m "$1"
  > }

Setup
  $ newclientrepo
  $ mkcommit root
  $ mkcommit public1
  $ mkcommit public2
  $ sl debugmakepublic .
  $ sl debugvisibility status
  commit visibility is tracked explicitly

Simple creation and amending of draft commits

  $ mkcommit draft1
  $ sl debugvisibleheads
  ca9d66205acae45570c29bea55877bb8031aa453 draft1
  $ sl amend -m "draft1 amend1"
  $ sl debugvisibleheads
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
  
  $ sl debugvisibleheads
  7ff6bbfeb971e769d9f5821bcdd4f42b446603d6 draft2

  $ sl debugstrip -r . --config amend.safestrip=False
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
  
  $ sl debugvisibleheads
  5b93956a25ec5ed476b39b46bbdd1efdfdf0ee6a draft1 amend1

# Quick test of children revsets when there is a hidden child.
  $ sl log -r 'desc("public2")~-1' -T '{desc}\n'
  draft1 amend1
  $ sl log -r 'children(desc("public2"))' -T '{desc}\n'
  draft1 amend1


  $ mkcommit draft2a
  $ sl rebase -s ".^" -d 'desc(public1)'
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
  
  $ sl debugvisibleheads
  6c98fad065b92f276ba38ad20df31312c0bcf342 draft2a
  $ sl rebase -s . -d 'desc(public2)'
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
  
  $ sl debugvisibleheads
  5b3624aa14e4e48b969670853d0a0eb95dfaa13a draft2a
  e500bf5639fd8c869a8cf4e10b651aaa01bfa42c draft1 amend1

Add more commits

  $ mkcommit draft3
  $ mkcommit draft4
  $ sl merge -q 'max(desc(draft1))'
  $ sl commit -m "merge1"
  $ sl up -q 'max(desc(draft1))'
  $ sl merge -q 'desc(draft4)'
  $ sl commit -m "merge2"
  $ sl debugmakepublic 'desc(root)'

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

  $ sl up -q 'desc(root)'
  $ sl hide 'desc(merge1)'
  hiding commit 6abdecc48cd1 "merge1"
  1 changeset hidden
  $ sl debugvisibleheads
  4daf58216052b220cae410be4e8607c11f7ad7c2 merge2
  $ sl hide 'max(desc(draft2a))'
  hiding commit 5b3624aa14e4 "draft2a"
  hiding commit 4975507bccc9 "draft3"
  hiding commit 02227f0b851e "draft4"
  hiding commit 4daf58216052 "merge2"
  4 changesets hidden
  $ sl debugvisibleheads
  e500bf5639fd8c869a8cf4e10b651aaa01bfa42c draft1 amend1
  4f416a252ac81004d9b35542cb1dc8892b6879eb public2
  $ sl unhide 'desc(draft3)'
  $ sl debugvisibleheads
  e500bf5639fd8c869a8cf4e10b651aaa01bfa42c draft1 amend1
  4975507bccc988c9fe1822b1ec33e754ae0b0334 draft3
  $ sl hide 'desc(public2)' 'desc(amend1)'
  hiding commit 4f416a252ac8 "public2"
  hiding commit e500bf5639fd "draft1 amend1"
  hiding commit 5b3624aa14e4 "draft2a"
  hiding commit 4975507bccc9 "draft3"
  4 changesets hidden
  $ sl debugvisibleheads
  175dbab47dccefd3ece5916c4f92a6c69f65fcf0 public1
  $ sl unhide 'max(desc(draft1))'
  $ sl debugvisibleheads
  e500bf5639fd8c869a8cf4e10b651aaa01bfa42c draft1 amend1
  $ sl hide 'desc(public1)'
  hiding commit 175dbab47dcc "public1"
  hiding commit e500bf5639fd "draft1 amend1"
  2 changesets hidden
  $ sl debugvisibleheads
  $ sl unhide 'desc(merge1)'
  $ sl debugvisibleheads
  6abdecc48cd1aa6c444bb3abed96fee7af294684 merge1
  $ sl unhide 'desc(merge2)'
  $ sl debugvisibleheads
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
  $ sl up $B
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ sl amend -m "B amended" --no-rebase
  hint[amend-restack]: descendants of 112478962961 are left behind - use 'sl restack' to rebase them
  hint[hint-ack]: use 'sl hint --ack amend-restack' to silence these hints
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
  
  $ sl next --rebase
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
  
  $ sl next --rebase
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
  
  $ sl next --rebase
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

  $ sl undo
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
  
  $ sl undo
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
  
  $ sl undo
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
  $ sl unhide ef1bf99db56b
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
  
  $ sl up -q 917a077edb8d # Update to B
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
  
  $ sl up -q $F
  $ tglogm
  @  12f43da6ed39 'F'
  │
  o  ec4d05032fe4 'E'
  │
  o  48b9aae0607f 'Z'
  
Test that shelve and unshelve work
  $ echo more > file
  $ sl add file
  $ sl st
  A file
  $ sl shelve
  shelved as default
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ sl st
  $ tglogm
  @  12f43da6ed39 'F'
  │
  o  ec4d05032fe4 'E'
  │
  o  48b9aae0607f 'Z'
  
  $ sl prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [ec4d05] E
  $ sl unshelve --keep
  unshelving change 'default'
  rebasing shelved changes
  rebasing 43c5c8656322 "shelve changes to: F"
  $ sl st
  A file
  $ tglogm
  o  12f43da6ed39 'F'
  │
  @  ec4d05032fe4 'E'
  │
  o  48b9aae0607f 'Z'
  
  $ sl prev --clean
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [48b9aa] Z
  $ echo data > other
  $ sl add other
  $ sl st
  A other
  ? file
  $ sl unshelve
  unshelving change 'default'
  temporarily committing pending changes (restore with 'sl unshelve --abort')
  rebasing shelved changes
  rebasing 43c5c8656322 "shelve changes to: F"
  $ sl st
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
  $ sl commit -Aqm base
  $ echo file1 > file1
  $ echo file2 > file2
  $ echo file3 > file3
  $ sl commit -Aqm to-split
  $ sl split --config ui.interactive=true << EOF
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
  
  $ sl undo
  undone to *, before split --config ui.interactive=true (glob)
  $ tglogm
  @  9a8c420e44f2 'to-split'
  │
  o  d20a80d4def3 'base'
  
Unamend and Uncommit
  $ cd $TESTTMP
  $ newclientrepo
  $ touch base
  $ sl commit -Aqm base
  $ echo 1 > file
  $ sl commit -Aqm commit1
  $ echo 2 > file
  $ sl amend -m commit2
  $ tglogm --hidden
  @  e70c2acd5a58 'commit2'
  │
  │ x  4c5b9b3e14b9 'commit1'  (Rewritten using amend into e70c2acd5a58)
  ├─╯
  o  df4f53cec30a 'base'
  

  $ sl unamend
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
  

  $ sl uncommit
  $ tglogm
  @  df4f53cec30a 'base'
  
  $ tglogm --hidden
  o  e70c2acd5a58 'commit2'
  │
  │ x  4c5b9b3e14b9 'commit1'  (Rewritten using amend into e70c2acd5a58)
  ├─╯
  @  df4f53cec30a 'base'
  

Hidden revset
  $ sl log --graph -r 'hidden()'
  o  commit:      e70c2acd5a58
  │  user:        test
  ~  date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     commit2
  
  o  commit:      4c5b9b3e14b9
  │  user:        test
  ~  date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     commit1
  
