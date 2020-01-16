#chg-compatible

  $ enable amend rebase undo directaccess shelve
  $ setconfig experimental.evolution=obsolete
  $ setconfig visibility.enabled=true
  $ setconfig mutation.record=true mutation.enabled=true mutation.date="0 0"
  $ setconfig hint.ack=undo

Useful functions
  $ mkcommit()
  > {
  >   echo "$1" > "$1"
  >   hg add "$1"
  >   hg commit -m "$1"
  > }

  $ printvisibleheads() {
  >   hg dbsh -c 'ui.write(repo.svfs.read("visibleheads"))' | sort
  > }

Setup
  $ newrepo
  $ mkcommit root
  $ mkcommit public1
  $ mkcommit public2
  $ hg phase -p .

Simple creation and amending of draft commits

  $ mkcommit draft1
  $ printvisibleheads
  ca9d66205acae45570c29bea55877bb8031aa453
  v1
  $ hg amend -m "draft1 amend1"
  $ printvisibleheads
  bc066ca12b451d14668c7a3e38757449b7d6a104
  v1
  $ mkcommit draft2
  $ tglogp --hidden
  @  5: 467d8aa13aef draft 'draft2'
  |
  o  4: bc066ca12b45 draft 'draft1 amend1'
  |
  | x  3: ca9d66205aca draft 'draft1'
  |/
  o  2: 4f416a252ac8 public 'public2'
  |
  o  1: 175dbab47dcc public 'public1'
  |
  o  0: 1e4be0697311 public 'root'
  
  $ printvisibleheads
  467d8aa13aef105d18160ea682d5cf20d8941d06
  v1

  $ hg debugstrip -r . --config amend.safestrip=False
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/* (glob)
  $ tglogp --hidden
  @  4: bc066ca12b45 draft 'draft1 amend1'
  |
  | x  3: ca9d66205aca draft 'draft1'
  |/
  o  2: 4f416a252ac8 public 'public2'
  |
  o  1: 175dbab47dcc public 'public1'
  |
  o  0: 1e4be0697311 public 'root'
  
  $ printvisibleheads
  bc066ca12b451d14668c7a3e38757449b7d6a104
  v1

  $ mkcommit draft2a
  $ hg rebase -s ".^" -d 1
  rebasing bc066ca12b45 "draft1 amend1"
  rebasing 2ccd7cddaa94 "draft2a"
  $ tglogp
  @  7: ecfc0c412bb8 draft 'draft2a'
  |
  o  6: 96b7359a7ee5 draft 'draft1 amend1'
  |
  | o  2: 4f416a252ac8 public 'public2'
  |/
  o  1: 175dbab47dcc public 'public1'
  |
  o  0: 1e4be0697311 public 'root'
  
  $ printvisibleheads
  ecfc0c412bb878c3e7b1b3468cae773b473fd3ec
  v1
  $ hg rebase -s . -d 2
  rebasing ecfc0c412bb8 "draft2a"
  $ tglogp
  @  8: af54c09bb37d draft 'draft2a'
  |
  | o  6: 96b7359a7ee5 draft 'draft1 amend1'
  | |
  o |  2: 4f416a252ac8 public 'public2'
  |/
  o  1: 175dbab47dcc public 'public1'
  |
  o  0: 1e4be0697311 public 'root'
  
  $ printvisibleheads
  96b7359a7ee5350b94be6e5c5dd480751a031498
  af54c09bb37da36975b8d482f660f62f95697a35
  v1

Simple phase adjustments

  $ hg phase -p 6
  $ printvisibleheads
  af54c09bb37da36975b8d482f660f62f95697a35
  v1
  $ hg phase -df 6
  $ printvisibleheads
  96b7359a7ee5350b94be6e5c5dd480751a031498
  af54c09bb37da36975b8d482f660f62f95697a35
  v1

  $ mkcommit draft3
  $ mkcommit draft4
  $ tglogp
  @  10: f3f5679a1c9c draft 'draft4'
  |
  o  9: 5dabc7b08ef9 draft 'draft3'
  |
  o  8: af54c09bb37d draft 'draft2a'
  |
  | o  6: 96b7359a7ee5 draft 'draft1 amend1'
  | |
  o |  2: 4f416a252ac8 public 'public2'
  |/
  o  1: 175dbab47dcc public 'public1'
  |
  o  0: 1e4be0697311 public 'root'
  
  $ printvisibleheads
  96b7359a7ee5350b94be6e5c5dd480751a031498
  f3f5679a1c9cb5a79334a3bbb87b359864c44ce4
  v1
  $ hg phase -p 9
  $ printvisibleheads
  96b7359a7ee5350b94be6e5c5dd480751a031498
  f3f5679a1c9cb5a79334a3bbb87b359864c44ce4
  v1
  $ hg phase -p 10
  $ printvisibleheads
  96b7359a7ee5350b94be6e5c5dd480751a031498
  v1
  $ hg phase -sf 9
  $ printvisibleheads
  96b7359a7ee5350b94be6e5c5dd480751a031498
  f3f5679a1c9cb5a79334a3bbb87b359864c44ce4
  v1
  $ hg phase -df 8
  $ printvisibleheads
  96b7359a7ee5350b94be6e5c5dd480751a031498
  f3f5679a1c9cb5a79334a3bbb87b359864c44ce4
  v1
  $ tglogp
  @  10: f3f5679a1c9c secret 'draft4'
  |
  o  9: 5dabc7b08ef9 secret 'draft3'
  |
  o  8: af54c09bb37d draft 'draft2a'
  |
  | o  6: 96b7359a7ee5 draft 'draft1 amend1'
  | |
  o |  2: 4f416a252ac8 public 'public2'
  |/
  o  1: 175dbab47dcc public 'public1'
  |
  o  0: 1e4be0697311 public 'root'
  
  $ hg merge -q 6
  $ hg commit -m "merge1"
  $ hg up -q 6
  $ hg merge -q 10
  $ hg commit -m "merge2"
  $ tglogp
  @    12: 8a541e4b5b52 secret 'merge2'
  |\
  +---o  11: 00c8b0f0741e secret 'merge1'
  | |/
  | o  10: f3f5679a1c9c secret 'draft4'
  | |
  | o  9: 5dabc7b08ef9 secret 'draft3'
  | |
  | o  8: af54c09bb37d draft 'draft2a'
  | |
  o |  6: 96b7359a7ee5 draft 'draft1 amend1'
  | |
  | o  2: 4f416a252ac8 public 'public2'
  |/
  o  1: 175dbab47dcc public 'public1'
  |
  o  0: 1e4be0697311 public 'root'
  
  $ printvisibleheads
  00c8b0f0741e6ef0696abd63aba22f3d49018b38
  8a541e4b5b528ca9db5d1f8afd4f2534fcd79527
  v1

  $ hg phase -p 11
  $ printvisibleheads
  8a541e4b5b528ca9db5d1f8afd4f2534fcd79527
  v1
  $ hg phase -p 12
  $ printvisibleheads
  v1
  $ hg phase -df 11
  $ printvisibleheads
  00c8b0f0741e6ef0696abd63aba22f3d49018b38
  v1
  $ hg phase -df 10
  $ printvisibleheads
  00c8b0f0741e6ef0696abd63aba22f3d49018b38
  8a541e4b5b528ca9db5d1f8afd4f2534fcd79527
  v1
  $ hg phase -df 1
  $ printvisibleheads
  00c8b0f0741e6ef0696abd63aba22f3d49018b38
  8a541e4b5b528ca9db5d1f8afd4f2534fcd79527
  v1
  $ tglogp
  @    12: 8a541e4b5b52 draft 'merge2'
  |\
  +---o  11: 00c8b0f0741e draft 'merge1'
  | |/
  | o  10: f3f5679a1c9c draft 'draft4'
  | |
  | o  9: 5dabc7b08ef9 draft 'draft3'
  | |
  | o  8: af54c09bb37d draft 'draft2a'
  | |
  o |  6: 96b7359a7ee5 draft 'draft1 amend1'
  | |
  | o  2: 4f416a252ac8 draft 'public2'
  |/
  o  1: 175dbab47dcc draft 'public1'
  |
  o  0: 1e4be0697311 public 'root'
  
Hide and unhide

  $ hg up -q 0
  $ hg hide 11
  hiding commit 00c8b0f0741e "merge1"
  1 changeset hidden
  $ printvisibleheads
  8a541e4b5b528ca9db5d1f8afd4f2534fcd79527
  v1
  $ hg hide 8
  hiding commit af54c09bb37d "draft2a"
  hiding commit 5dabc7b08ef9 "draft3"
  hiding commit f3f5679a1c9c "draft4"
  hiding commit 8a541e4b5b52 "merge2"
  4 changesets hidden
  $ printvisibleheads
  4f416a252ac81004d9b35542cb1dc8892b6879eb
  96b7359a7ee5350b94be6e5c5dd480751a031498
  v1
  $ hg unhide 9
  $ printvisibleheads
  5dabc7b08ef934b9e6720285205b2c17695f6491
  96b7359a7ee5350b94be6e5c5dd480751a031498
  v1
  $ hg hide 2 6
  hiding commit 4f416a252ac8 "public2"
  hiding commit 96b7359a7ee5 "draft1 amend1"
  hiding commit af54c09bb37d "draft2a"
  hiding commit 5dabc7b08ef9 "draft3"
  4 changesets hidden
  $ printvisibleheads
  175dbab47dccefd3ece5916c4f92a6c69f65fcf0
  v1
  $ hg unhide 6
  $ printvisibleheads
  96b7359a7ee5350b94be6e5c5dd480751a031498
  v1
  $ hg hide 1
  hiding commit 175dbab47dcc "public1"
  hiding commit 96b7359a7ee5 "draft1 amend1"
  2 changesets hidden
  $ printvisibleheads
  v1
  $ hg unhide 11
  $ printvisibleheads
  00c8b0f0741e6ef0696abd63aba22f3d49018b38
  v1
  $ hg unhide 12
  $ printvisibleheads
  00c8b0f0741e6ef0696abd63aba22f3d49018b38
  8a541e4b5b528ca9db5d1f8afd4f2534fcd79527
  v1

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
  @  5: e60094faeb72 'B amended'
  |
  | o  4: 9bc730a19041 'E'
  | |
  | o  3: f585351a92f8 'D'
  | |
  | o  2: 26805aba1e60 'C'
  | |
  | x  1: 112478962961 'B'  (Rewritten using amend into e60094faeb72)
  |/
  o  0: 426bada5c675 'A'
  
  $ hg next --rebase
  rebasing 26805aba1e60 "C"
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [23910a] C
  $ tglogm
  @  6: 23910a6fe564 'C'
  |
  o  5: e60094faeb72 'B amended'
  |
  | o  4: 9bc730a19041 'E'
  | |
  | o  3: f585351a92f8 'D'
  | |
  | x  2: 26805aba1e60 'C'  (Rewritten using rebase into 23910a6fe564)
  | |
  | x  1: 112478962961 'B'  (Rewritten using amend into e60094faeb72)
  |/
  o  0: 426bada5c675 'A'
  
  $ hg next --rebase
  rebasing f585351a92f8 "D"
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [1d30cc] D
  $ tglogm
  @  7: 1d30cc995ea7 'D'
  |
  o  6: 23910a6fe564 'C'
  |
  o  5: e60094faeb72 'B amended'
  |
  | o  4: 9bc730a19041 'E'
  | |
  | x  3: f585351a92f8 'D'  (Rewritten using rebase into 1d30cc995ea7)
  | |
  | x  2: 26805aba1e60 'C'  (Rewritten using rebase into 23910a6fe564)
  | |
  | x  1: 112478962961 'B'  (Rewritten using amend into e60094faeb72)
  |/
  o  0: 426bada5c675 'A'
  
  $ hg next --rebase
  rebasing 9bc730a19041 "E"
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [ec992f] E
  $ tglogm
  @  8: ec992ff1fd78 'E'
  |
  o  7: 1d30cc995ea7 'D'
  |
  o  6: 23910a6fe564 'C'
  |
  o  5: e60094faeb72 'B amended'
  |
  o  0: 426bada5c675 'A'
  

Undo

  $ hg undo
  undone to *, before next --rebase (glob)
  $ tglogm
  @  7: 1d30cc995ea7 'D'
  |
  o  6: 23910a6fe564 'C'
  |
  o  5: e60094faeb72 'B amended'
  |
  | o  4: 9bc730a19041 'E'
  | |
  | x  3: f585351a92f8 'D'  (Rewritten using rebase into 1d30cc995ea7)
  | |
  | x  2: 26805aba1e60 'C'  (Rewritten using rebase into 23910a6fe564)
  | |
  | x  1: 112478962961 'B'  (Rewritten using amend into e60094faeb72)
  |/
  o  0: 426bada5c675 'A'
  
  $ hg undo
  undone to *, before next --rebase (glob)
  $ tglogm
  @  6: 23910a6fe564 'C'
  |
  o  5: e60094faeb72 'B amended'
  |
  | o  4: 9bc730a19041 'E'
  | |
  | o  3: f585351a92f8 'D'
  | |
  | x  2: 26805aba1e60 'C'  (Rewritten using rebase into 23910a6fe564)
  | |
  | x  1: 112478962961 'B'  (Rewritten using amend into e60094faeb72)
  |/
  o  0: 426bada5c675 'A'
  
  $ hg undo
  undone to *, before next --rebase (glob)
  $ tglogm
  @  5: e60094faeb72 'B amended'
  |
  | o  4: 9bc730a19041 'E'
  | |
  | o  3: f585351a92f8 'D'
  | |
  | o  2: 26805aba1e60 'C'
  | |
  | x  1: 112478962961 'B'  (Rewritten using amend into e60094faeb72)
  |/
  o  0: 426bada5c675 'A'
  
Viewing the log graph with filtering disabled shows the commits that have been undone
from as invisible commits.
  $ tglogm --hidden
  x  8: ec992ff1fd78 'E'
  |
  x  7: 1d30cc995ea7 'D'
  |
  x  6: 23910a6fe564 'C'
  |
  @  5: e60094faeb72 'B amended'
  |
  | o  4: 9bc730a19041 'E'
  | |
  | o  3: f585351a92f8 'D'
  | |
  | o  2: 26805aba1e60 'C'
  | |
  | x  1: 112478962961 'B'  (Rewritten using amend into e60094faeb72)
  |/
  o  0: 426bada5c675 'A'
  
Also check the obsolete revset is consistent.
  $ tglogm -r "obsolete()"
  x  1: 112478962961 'B'  (Rewritten using amend into e60094faeb72)
  |
  ~
  $ tglogm --hidden -r "obsolete()"
  x  1: 112478962961 'B'  (Rewritten using amend into e60094faeb72)
  |
  ~

Unhiding them reveals them as new commits and now the old ones show their relationship
to the new ones.
  $ hg unhide ec992ff1fd78
  $ tglogm
  o  8: ec992ff1fd78 'E'
  |
  o  7: 1d30cc995ea7 'D'
  |
  o  6: 23910a6fe564 'C'
  |
  @  5: e60094faeb72 'B amended'
  |
  | x  4: 9bc730a19041 'E'  (Rewritten using rebase into ec992ff1fd78)
  | |
  | x  3: f585351a92f8 'D'  (Rewritten using rebase into 1d30cc995ea7)
  | |
  | x  2: 26805aba1e60 'C'  (Rewritten using rebase into 23910a6fe564)
  | |
  | x  1: 112478962961 'B'  (Rewritten using amend into e60094faeb72)
  |/
  o  0: 426bada5c675 'A'
  
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
  o  6: a77c932a84af 'F'
  |
  o  5: 05eb30556340 'E'
  |
  o  0: 48b9aae0607f 'Z'
  
  $ hg up -q 917a077edb8d # Update to B
  $ tglogm
  o  6: a77c932a84af 'F'
  |
  o  5: 05eb30556340 'E'
  |
  | @  2: 917a077edb8d 'B'  (Rewritten using rewrite into a77c932a84af)
  | |
  | x  1: ac2f7407182b 'A'  (Rewritten using rewrite into 05eb30556340)
  |/
  o  0: 48b9aae0607f 'Z'
  
  $ hg up -q $F
  $ tglogm
  @  6: a77c932a84af 'F'
  |
  o  5: 05eb30556340 'E'
  |
  o  0: 48b9aae0607f 'Z'
  
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
  @  6: a77c932a84af 'F'
  |
  o  5: 05eb30556340 'E'
  |
  o  0: 48b9aae0607f 'Z'
  
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
  o  6: a77c932a84af 'F'
  |
  @  5: 05eb30556340 'E'
  |
  o  0: 48b9aae0607f 'Z'
  
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
  o  6: a77c932a84af 'F'
  |
  o  5: 05eb30556340 'E'
  |
  @  0: 48b9aae0607f 'Z'
  
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
  @  4: a30320c497f0 'to-split'
  |
  o  3: 0a2500cbe503 'to-split'
  |
  o  2: 06e40e6ae08c 'to-split'
  |
  o  0: d20a80d4def3 'base'
  
  $ hg undo
  undone to *, before split --config ui.interactive=true (glob)
  $ tglogm
  @  1: 9a8c420e44f2 'to-split'
  |
  o  0: d20a80d4def3 'base'
  
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
  @  2: 8e8ec65c0bb7 'commit2'
  |
  | x  1: 4c5b9b3e14b9 'commit1'  (Rewritten using amend into 8e8ec65c0bb7)
  |/
  o  0: df4f53cec30a 'base'
  

  $ hg unamend
  $ tglogm
  @  1: 4c5b9b3e14b9 'commit1'
  |
  o  0: df4f53cec30a 'base'
  
  $ tglogm --hidden
  x  2: 8e8ec65c0bb7 'commit2'
  |
  | @  1: 4c5b9b3e14b9 'commit1'
  |/
  o  0: df4f53cec30a 'base'
  

  $ hg uncommit
  $ tglogm
  @  0: df4f53cec30a 'base'
  
  $ tglogm --hidden
  x  2: 8e8ec65c0bb7 'commit2'
  |
  | x  1: 4c5b9b3e14b9 'commit1'
  |/
  @  0: df4f53cec30a 'base'
  
