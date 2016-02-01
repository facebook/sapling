  $ . "$TESTDIR/histedit-helpers.sh"

  $ cat >> $HGRCPATH <<EOF
  > [alias]
  > tglog = log -G --template "{rev}:{node}:{phase} '{desc}'\n"
  > [extensions]
  > histedit=
  > [experimental]
  > histeditng=True
  > EOF

Create repo a:

  $ hg init a
  $ cd a
  $ hg unbundle "$TESTDIR/bundles/rebase.hg"
  adding changesets
  adding manifests
  adding file changes
  added 8 changesets with 7 changes to 7 files (+2 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hg up tip
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg tglog
  @  7:02de42196ebee42ef284b6780a87cdc96e8eaab6:draft 'H'
  |
  | o  6:eea13746799a9e0bfd88f29d3c2e9dc9389f524f:draft 'G'
  |/|
  o |  5:24b6387c8c8cae37178880f3fa95ded3cb1cf785:draft 'F'
  | |
  | o  4:9520eea781bcca16c1e15acc0ba14335a0e8e5ba:draft 'E'
  |/
  | o  3:32af7686d403cf45b5d95f2d70cebea587ac806a:draft 'D'
  | |
  | o  2:5fddd98957c8a54a4d436dfe1da9d87f21a1b97b:draft 'C'
  | |
  | o  1:42ccdea3bb16d28e1848c95fe2e44c000f3f21b1:draft 'B'
  |/
  o  0:cd010b8cd998f3981a5a8115f94f8da4ab506089:draft 'A'
  


Go to D
  $ hg update 3
  3 files updated, 0 files merged, 2 files removed, 0 files unresolved
edit the history to rebase B onto H


Rebase B onto H
  $ hg histedit 1 --commands - 2>&1 << EOF | fixbundle
  > base 02de42196ebe
  > pick 42ccdea3bb16 B
  > pick 5fddd98957c8 C
  > pick 32af7686d403 D
  > EOF

  $ hg tglog
  @  7:0937e82309df47d14176ee15e45dbec5fbdef340:draft 'D'
  |
  o  6:f778d1cbddac4ab679d9983c9bb92e4c5e09e7fa:draft 'C'
  |
  o  5:3d41b7cc708545206213a842f96d812d2e73d818:draft 'B'
  |
  o  4:02de42196ebee42ef284b6780a87cdc96e8eaab6:draft 'H'
  |
  | o  3:eea13746799a9e0bfd88f29d3c2e9dc9389f524f:draft 'G'
  |/|
  o |  2:24b6387c8c8cae37178880f3fa95ded3cb1cf785:draft 'F'
  | |
  | o  1:9520eea781bcca16c1e15acc0ba14335a0e8e5ba:draft 'E'
  |/
  o  0:cd010b8cd998f3981a5a8115f94f8da4ab506089:draft 'A'
  
Rebase back and drop something
  $ hg histedit 5 --commands - 2>&1 << EOF | fixbundle
  > base cd010b8cd998
  > pick 3d41b7cc7085 B
  > drop f778d1cbddac C
  > pick 0937e82309df D
  > EOF

  $ hg tglog
  @  6:476cc3e4168da2d036b141f7f7dcff7f8e3fe846:draft 'D'
  |
  o  5:d273e35dcdf21a7eb305192ef2e362887cd0a6f8:draft 'B'
  |
  | o  4:02de42196ebee42ef284b6780a87cdc96e8eaab6:draft 'H'
  | |
  | | o  3:eea13746799a9e0bfd88f29d3c2e9dc9389f524f:draft 'G'
  | |/|
  | o |  2:24b6387c8c8cae37178880f3fa95ded3cb1cf785:draft 'F'
  |/ /
  | o  1:9520eea781bcca16c1e15acc0ba14335a0e8e5ba:draft 'E'
  |/
  o  0:cd010b8cd998f3981a5a8115f94f8da4ab506089:draft 'A'
  
Split stack
  $ hg histedit 5 --commands - 2>&1 << EOF | fixbundle
  > base cd010b8cd998
  > pick d273e35dcdf2 B
  > base cd010b8cd998
  > pick 476cc3e4168d D
  > EOF

  $ hg tglog
  @  6:d7a6f907a822c4ce6f15662ae45a42aa46d3818a:draft 'D'
  |
  | o  5:d273e35dcdf21a7eb305192ef2e362887cd0a6f8:draft 'B'
  |/
  | o  4:02de42196ebee42ef284b6780a87cdc96e8eaab6:draft 'H'
  | |
  | | o  3:eea13746799a9e0bfd88f29d3c2e9dc9389f524f:draft 'G'
  | |/|
  | o |  2:24b6387c8c8cae37178880f3fa95ded3cb1cf785:draft 'F'
  |/ /
  | o  1:9520eea781bcca16c1e15acc0ba14335a0e8e5ba:draft 'E'
  |/
  o  0:cd010b8cd998f3981a5a8115f94f8da4ab506089:draft 'A'
  
Abort
  $ echo x > B
  $ hg add B
  $ hg commit -m "X"
  $ hg tglog
  @  7:591369deedfdcbf57471e894999a70d7f676186d:draft 'X'
  |
  o  6:d7a6f907a822c4ce6f15662ae45a42aa46d3818a:draft 'D'
  |
  | o  5:d273e35dcdf21a7eb305192ef2e362887cd0a6f8:draft 'B'
  |/
  | o  4:02de42196ebee42ef284b6780a87cdc96e8eaab6:draft 'H'
  | |
  | | o  3:eea13746799a9e0bfd88f29d3c2e9dc9389f524f:draft 'G'
  | |/|
  | o |  2:24b6387c8c8cae37178880f3fa95ded3cb1cf785:draft 'F'
  |/ /
  | o  1:9520eea781bcca16c1e15acc0ba14335a0e8e5ba:draft 'E'
  |/
  o  0:cd010b8cd998f3981a5a8115f94f8da4ab506089:draft 'A'
  
  $ hg histedit 6 --commands - 2>&1 << EOF | fixbundle
  > base d273e35dcdf2 B
  > drop d7a6f907a822 D
  > pick 591369deedfd X
  > EOF
  merging B
  warning: conflicts while merging B! (edit, then use 'hg resolve --mark')
  Fix up the change (pick 591369deedfd)
  (hg histedit --continue to resume)
  $ hg histedit --abort | fixbundle
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg tglog
  @  7:591369deedfdcbf57471e894999a70d7f676186d:draft 'X'
  |
  o  6:d7a6f907a822c4ce6f15662ae45a42aa46d3818a:draft 'D'
  |
  | o  5:d273e35dcdf21a7eb305192ef2e362887cd0a6f8:draft 'B'
  |/
  | o  4:02de42196ebee42ef284b6780a87cdc96e8eaab6:draft 'H'
  | |
  | | o  3:eea13746799a9e0bfd88f29d3c2e9dc9389f524f:draft 'G'
  | |/|
  | o |  2:24b6387c8c8cae37178880f3fa95ded3cb1cf785:draft 'F'
  |/ /
  | o  1:9520eea781bcca16c1e15acc0ba14335a0e8e5ba:draft 'E'
  |/
  o  0:cd010b8cd998f3981a5a8115f94f8da4ab506089:draft 'A'
  
Continue
  $ hg histedit 6 --commands - 2>&1 << EOF | fixbundle
  > base d273e35dcdf2 B
  > drop d7a6f907a822 D
  > pick 591369deedfd X
  > EOF
  merging B
  warning: conflicts while merging B! (edit, then use 'hg resolve --mark')
  Fix up the change (pick 591369deedfd)
  (hg histedit --continue to resume)
  $ echo b2 > B
  $ hg resolve --mark B
  (no more unresolved files)
  continue: hg histedit --continue
  $ hg histedit --continue | fixbundle
  $ hg tglog
  @  6:03772da75548bb42a8f1eacd8c91d0717a147fcd:draft 'X'
  |
  o  5:d273e35dcdf21a7eb305192ef2e362887cd0a6f8:draft 'B'
  |
  | o  4:02de42196ebee42ef284b6780a87cdc96e8eaab6:draft 'H'
  | |
  | | o  3:eea13746799a9e0bfd88f29d3c2e9dc9389f524f:draft 'G'
  | |/|
  | o |  2:24b6387c8c8cae37178880f3fa95ded3cb1cf785:draft 'F'
  |/ /
  | o  1:9520eea781bcca16c1e15acc0ba14335a0e8e5ba:draft 'E'
  |/
  o  0:cd010b8cd998f3981a5a8115f94f8da4ab506089:draft 'A'
  

base on a previously picked changeset
  $ echo i > i
  $ hg add i
  $ hg commit -m "I"
  $ echo j > j
  $ hg add j
  $ hg commit -m "J"
  $ hg tglog
  @  8:e8c55b19d366b335626e805484110d1d5f6f2ea3:draft 'J'
  |
  o  7:b2f90fd8aa85db5569e3cfc30cd1d7739546368e:draft 'I'
  |
  o  6:03772da75548bb42a8f1eacd8c91d0717a147fcd:draft 'X'
  |
  o  5:d273e35dcdf21a7eb305192ef2e362887cd0a6f8:draft 'B'
  |
  | o  4:02de42196ebee42ef284b6780a87cdc96e8eaab6:draft 'H'
  | |
  | | o  3:eea13746799a9e0bfd88f29d3c2e9dc9389f524f:draft 'G'
  | |/|
  | o |  2:24b6387c8c8cae37178880f3fa95ded3cb1cf785:draft 'F'
  |/ /
  | o  1:9520eea781bcca16c1e15acc0ba14335a0e8e5ba:draft 'E'
  |/
  o  0:cd010b8cd998f3981a5a8115f94f8da4ab506089:draft 'A'
  
  $ hg histedit 5 --commands - 2>&1 << EOF | fixbundle
  > pick d273e35dcdf2 B
  > pick 03772da75548 X
  > base d273e35dcdf2 B
  > pick e8c55b19d366 J
  > base d273e35dcdf2 B
  > pick b2f90fd8aa85 I
  > EOF
  hg: parse error: base "d273e35dcdf2" changeset was not an edited list candidate
  (only use listed changesets)

  $ hg --config experimental.histeditng=False histedit 5 --commands - 2>&1 << EOF | fixbundle
  > base cd010b8cd998 A
  > pick d273e35dcdf2 B
  > pick 03772da75548 X
  > pick b2f90fd8aa85 I
  > pick e8c55b19d366 J
  > EOF
  hg: parse error: unknown action "base"

  $ hg tglog
  @  8:e8c55b19d366b335626e805484110d1d5f6f2ea3:draft 'J'
  |
  o  7:b2f90fd8aa85db5569e3cfc30cd1d7739546368e:draft 'I'
  |
  o  6:03772da75548bb42a8f1eacd8c91d0717a147fcd:draft 'X'
  |
  o  5:d273e35dcdf21a7eb305192ef2e362887cd0a6f8:draft 'B'
  |
  | o  4:02de42196ebee42ef284b6780a87cdc96e8eaab6:draft 'H'
  | |
  | | o  3:eea13746799a9e0bfd88f29d3c2e9dc9389f524f:draft 'G'
  | |/|
  | o |  2:24b6387c8c8cae37178880f3fa95ded3cb1cf785:draft 'F'
  |/ /
  | o  1:9520eea781bcca16c1e15acc0ba14335a0e8e5ba:draft 'E'
  |/
  o  0:cd010b8cd998f3981a5a8115f94f8da4ab506089:draft 'A'
  
