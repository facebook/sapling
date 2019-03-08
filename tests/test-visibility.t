  $ enable amend rebase
  $ setconfig experimental.evolution=
  $ setconfig visibility.tracking=on
  $ setconfig mutation.record=true mutation.enabled=true mutation.date="0 0"

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
  $ hg phase -p .

Simple creation and amending of draft commits

  $ mkcommit draft1
  $ sort < .hg/store/visibleheads
  ca9d66205acae45570c29bea55877bb8031aa453
  v1
  $ hg amend -m "draft1 amend1"
  $ sort < .hg/store/visibleheads
  492be1647de8620d0b468b7948cd9d3cef868d39
  v1
  $ mkcommit draft2
  $ tglogp --hidden
  @  5: dd47114a5019 draft 'draft2'
  |
  o  4: 492be1647de8 draft 'draft1 amend1'
  |
  | x  3: ca9d66205aca draft 'draft1'
  |/
  o  2: 4f416a252ac8 public 'public2'
  |
  o  1: 175dbab47dcc public 'public1'
  |
  o  0: 1e4be0697311 public 'root'
  
  $ sort < .hg/store/visibleheads
  dd47114a501991a983cb0c412ad60653386bc29c
  v1

  $ hg debugstrip -r . --config amend.safestrip=False
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/* (glob)
  $ tglogp --hidden
  @  4: 492be1647de8 draft 'draft1 amend1'
  |
  | x  3: ca9d66205aca draft 'draft1'
  |/
  o  2: 4f416a252ac8 public 'public2'
  |
  o  1: 175dbab47dcc public 'public1'
  |
  o  0: 1e4be0697311 public 'root'
  
  $ sort < .hg/store/visibleheads
  492be1647de8620d0b468b7948cd9d3cef868d39
  v1

  $ mkcommit draft2a
  $ hg rebase -s ".^" -d 1
  rebasing 4:492be1647de8 "draft1 amend1"
  rebasing 5:10bf4507befa "draft2a" (tip)
  $ tglogp
  @  7: e1d231b878f2 draft 'draft2a'
  |
  o  6: 5fc86ee2448a draft 'draft1 amend1'
  |
  | o  2: 4f416a252ac8 public 'public2'
  |/
  o  1: 175dbab47dcc public 'public1'
  |
  o  0: 1e4be0697311 public 'root'
  
  $ sort < .hg/store/visibleheads
  e1d231b878f2f175c6d17bf6aa13c05a511d9813
  v1
  $ hg rebase -s . -d 2
  rebasing 7:e1d231b878f2 "draft2a" (tip)
  $ tglogp
  @  8: 4985030cb61e draft 'draft2a'
  |
  | o  6: 5fc86ee2448a draft 'draft1 amend1'
  | |
  o |  2: 4f416a252ac8 public 'public2'
  |/
  o  1: 175dbab47dcc public 'public1'
  |
  o  0: 1e4be0697311 public 'root'
  
  $ sort < .hg/store/visibleheads
  4985030cb61e3c2980862680502dc3185c10fbcd
  5fc86ee2448abe888ca0aacfc6def87882fee994
  v1

Simple phase adjustments

  $ hg phase -p 6
  $ sort < .hg/store/visibleheads
  4985030cb61e3c2980862680502dc3185c10fbcd
  v1
  $ hg phase -df 6
  $ sort < .hg/store/visibleheads
  4985030cb61e3c2980862680502dc3185c10fbcd
  5fc86ee2448abe888ca0aacfc6def87882fee994
  v1

  $ mkcommit draft3
  $ mkcommit draft4
  $ tglogp
  @  10: 685d49b9a3be draft 'draft4'
  |
  o  9: 2893394e4a13 draft 'draft3'
  |
  o  8: 4985030cb61e draft 'draft2a'
  |
  | o  6: 5fc86ee2448a draft 'draft1 amend1'
  | |
  o |  2: 4f416a252ac8 public 'public2'
  |/
  o  1: 175dbab47dcc public 'public1'
  |
  o  0: 1e4be0697311 public 'root'
  
  $ sort < .hg/store/visibleheads
  5fc86ee2448abe888ca0aacfc6def87882fee994
  685d49b9a3bee3b19da1b52e461b9f616fa03b85
  v1
  $ hg phase -p 9
  $ sort < .hg/store/visibleheads
  5fc86ee2448abe888ca0aacfc6def87882fee994
  685d49b9a3bee3b19da1b52e461b9f616fa03b85
  v1
  $ hg phase -p 10
  $ sort < .hg/store/visibleheads
  5fc86ee2448abe888ca0aacfc6def87882fee994
  v1
  $ hg phase -sf 9
  $ sort < .hg/store/visibleheads
  5fc86ee2448abe888ca0aacfc6def87882fee994
  685d49b9a3bee3b19da1b52e461b9f616fa03b85
  v1
  $ hg phase -df 8
  $ sort < .hg/store/visibleheads
  5fc86ee2448abe888ca0aacfc6def87882fee994
  685d49b9a3bee3b19da1b52e461b9f616fa03b85
  v1
  $ tglogp
  @  10: 685d49b9a3be secret 'draft4'
  |
  o  9: 2893394e4a13 secret 'draft3'
  |
  o  8: 4985030cb61e draft 'draft2a'
  |
  | o  6: 5fc86ee2448a draft 'draft1 amend1'
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
  @    12: a091c732061e secret 'merge2'
  |\
  +---o  11: 1fe51d588234 secret 'merge1'
  | |/
  | o  10: 685d49b9a3be secret 'draft4'
  | |
  | o  9: 2893394e4a13 secret 'draft3'
  | |
  | o  8: 4985030cb61e draft 'draft2a'
  | |
  o |  6: 5fc86ee2448a draft 'draft1 amend1'
  | |
  | o  2: 4f416a252ac8 public 'public2'
  |/
  o  1: 175dbab47dcc public 'public1'
  |
  o  0: 1e4be0697311 public 'root'
  
  $ sort < .hg/store/visibleheads
  1fe51d58823409175cc32bfbe9092540e1a8d294
  a091c732061ecac40da7a627cdb3008e8ef4680f
  v1

  $ hg phase -p 11
  $ sort < .hg/store/visibleheads
  a091c732061ecac40da7a627cdb3008e8ef4680f
  v1
  $ hg phase -p 12
  $ sort < .hg/store/visibleheads
  v1
  $ hg phase -df 11
  $ sort < .hg/store/visibleheads
  1fe51d58823409175cc32bfbe9092540e1a8d294
  v1
  $ hg phase -df 10
  $ sort < .hg/store/visibleheads
  1fe51d58823409175cc32bfbe9092540e1a8d294
  a091c732061ecac40da7a627cdb3008e8ef4680f
  v1
  $ hg phase -df 1
  $ sort < .hg/store/visibleheads
  1fe51d58823409175cc32bfbe9092540e1a8d294
  a091c732061ecac40da7a627cdb3008e8ef4680f
  v1
  $ tglogp
  @    12: a091c732061e draft 'merge2'
  |\
  +---o  11: 1fe51d588234 draft 'merge1'
  | |/
  | o  10: 685d49b9a3be draft 'draft4'
  | |
  | o  9: 2893394e4a13 draft 'draft3'
  | |
  | o  8: 4985030cb61e draft 'draft2a'
  | |
  o |  6: 5fc86ee2448a draft 'draft1 amend1'
  | |
  | o  2: 4f416a252ac8 draft 'public2'
  |/
  o  1: 175dbab47dcc draft 'public1'
  |
  o  0: 1e4be0697311 public 'root'
  
Hide and unhide

  $ hg up -q 0
  $ hg hide 11
  hiding commit 1fe51d588234 "merge1"
  1 changesets hidden
  $ sort < .hg/store/visibleheads
  a091c732061ecac40da7a627cdb3008e8ef4680f
  v1
  $ hg hide 8
  hiding commit 4985030cb61e "draft2a"
  hiding commit 2893394e4a13 "draft3"
  hiding commit 685d49b9a3be "draft4"
  hiding commit a091c732061e "merge2"
  4 changesets hidden
  $ sort < .hg/store/visibleheads
  4f416a252ac81004d9b35542cb1dc8892b6879eb
  5fc86ee2448abe888ca0aacfc6def87882fee994
  v1
  $ hg unhide 9
  $ sort < .hg/store/visibleheads
  2893394e4a132dc350d18e58e3407e0e09c40f50
  5fc86ee2448abe888ca0aacfc6def87882fee994
  v1
  $ hg hide 2 6
  hiding commit 4f416a252ac8 "public2"
  hiding commit 5fc86ee2448a "draft1 amend1"
  hiding commit 4985030cb61e "draft2a"
  hiding commit 2893394e4a13 "draft3"
  4 changesets hidden
  $ sort < .hg/store/visibleheads
  175dbab47dccefd3ece5916c4f92a6c69f65fcf0
  v1
  $ hg unhide 6
  $ sort < .hg/store/visibleheads
  5fc86ee2448abe888ca0aacfc6def87882fee994
  v1
  $ hg hide 1
  hiding commit 175dbab47dcc "public1"
  hiding commit 5fc86ee2448a "draft1 amend1"
  2 changesets hidden
  $ sort < .hg/store/visibleheads
  v1
  $ hg unhide 11
  $ sort < .hg/store/visibleheads
  1fe51d58823409175cc32bfbe9092540e1a8d294
  v1
  $ hg unhide 12
  $ sort < .hg/store/visibleheads
  1fe51d58823409175cc32bfbe9092540e1a8d294
  a091c732061ecac40da7a627cdb3008e8ef4680f
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
  $ tglog
  @  5: b233f9788d1b 'B amended'
  |
  | o  4: 9bc730a19041 'E'
  | |
  | o  3: f585351a92f8 'D'
  | |
  | o  2: 26805aba1e60 'C'
  | |
  | x  1: 112478962961 'B'
  |/
  o  0: 426bada5c675 'A'
  
  $ hg next --rebase
  rebasing 2:26805aba1e60 "C"
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [a7e6d0] C
  $ tglog
  @  6: a7e6d053d5ff 'C'
  |
  o  5: b233f9788d1b 'B amended'
  |
  | o  4: 9bc730a19041 'E'
  | |
  | o  3: f585351a92f8 'D'
  | |
  | x  2: 26805aba1e60 'C'
  | |
  | x  1: 112478962961 'B'
  |/
  o  0: 426bada5c675 'A'
  
  $ hg next --rebase
  rebasing 3:f585351a92f8 "D"
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [7e1a7a] D
  $ tglog
  @  7: 7e1a7a6f8549 'D'
  |
  o  6: a7e6d053d5ff 'C'
  |
  o  5: b233f9788d1b 'B amended'
  |
  | o  4: 9bc730a19041 'E'
  | |
  | x  3: f585351a92f8 'D'
  | |
  | x  2: 26805aba1e60 'C'
  | |
  | x  1: 112478962961 'B'
  |/
  o  0: 426bada5c675 'A'
  
  $ hg next --rebase
  rebasing 4:9bc730a19041 "E"
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [4547ce] E
  $ tglog
  @  8: 4547ceca9959 'E'
  |
  o  7: 7e1a7a6f8549 'D'
  |
  o  6: a7e6d053d5ff 'C'
  |
  o  5: b233f9788d1b 'B amended'
  |
  o  0: 426bada5c675 'A'
  
