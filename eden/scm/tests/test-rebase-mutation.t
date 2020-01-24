#chg-compatible

  $ disable treemanifest
  $ . helpers-usechg.sh

  $ configure mutation-norecord
  $ enable rebase
  $ setconfig phases.publish=false
  $ readconfig <<EOF
  > [ui]
  > logtemplate= {rev}:{node|short} {desc|firstline}{if(obsolete,' {mutation_nodes}')}
  > [templatealias]
  > mutation_nodes = "{join(mutations % '(rewritten using {operation} as {join(successors % \'{rev}:{node|short}\', \', \')})', ' ')}"
  > EOF

Setup rebase canonical repo

  $ hg init base
  $ cd base
  $ hg unbundle "$TESTDIR/bundles/rebase.hg"
  adding changesets
  adding manifests
  adding file changes
  added 8 changesets with 7 changes to 7 files
  $ hg up tip
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log -G
  @  7:02de42196ebe H
  |
  | o  6:eea13746799a G
  |/|
  o |  5:24b6387c8c8c F
  | |
  | o  4:9520eea781bc E
  |/
  | o  3:32af7686d403 D
  | |
  | o  2:5fddd98957c8 C
  | |
  | o  1:42ccdea3bb16 B
  |/
  o  0:cd010b8cd998 A
  
  $ cd ..

simple rebase
---------------------------------

  $ hg clone base simple
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd simple
  $ hg up 32af7686d403
  3 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg rebase -d eea13746799a
  rebasing 42ccdea3bb16 "B"
  rebasing 5fddd98957c8 "C"
  rebasing 32af7686d403 "D"
  $ hg log -G
  @  10:8eeb3c33ad33 D
  |
  o  9:2327fea05063 C
  |
  o  8:e4e5be0395b2 B
  |
  | o  7:02de42196ebe H
  | |
  o |  6:eea13746799a G
  |\|
  | o  5:24b6387c8c8c F
  | |
  o |  4:9520eea781bc E
  |/
  o  0:cd010b8cd998 A
  
  $ hg log --hidden -G
  @  10:8eeb3c33ad33 D
  |
  o  9:2327fea05063 C
  |
  o  8:e4e5be0395b2 B
  |
  | o  7:02de42196ebe H
  | |
  o |  6:eea13746799a G
  |\|
  | o  5:24b6387c8c8c F
  | |
  o |  4:9520eea781bc E
  |/
  | x  3:32af7686d403 D (rewritten using rebase as 10:8eeb3c33ad33)
  | |
  | x  2:5fddd98957c8 C (rewritten using rebase as 9:2327fea05063)
  | |
  | x  1:42ccdea3bb16 B (rewritten using rebase as 8:e4e5be0395b2)
  |/
  o  0:cd010b8cd998 A
  
  $ hg debugmutation ::.
   *  cd010b8cd998f3981a5a8115f94f8da4ab506089
   *  9520eea781bcca16c1e15acc0ba14335a0e8e5ba
   *  24b6387c8c8cae37178880f3fa95ded3cb1cf785
   *  eea13746799a9e0bfd88f29d3c2e9dc9389f524f
   *  e4e5be0395b2cbd471ed22a26b1b6a1a0658a794 rebase by test at 1970-01-01T00:00:00 from:
      42ccdea3bb16d28e1848c95fe2e44c000f3f21b1
   *  2327fea05063f39961b14cb69435a9898dc9a245 rebase by test at 1970-01-01T00:00:00 from:
      5fddd98957c8a54a4d436dfe1da9d87f21a1b97b
   *  8eeb3c33ad33d452c89e5dcf611c347f978fb42b rebase by test at 1970-01-01T00:00:00 from:
      32af7686d403cf45b5d95f2d70cebea587ac806a

  $ cd ..

empty changeset
---------------------------------

  $ hg clone base empty
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd empty
  $ hg up eea13746799a
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved

We make a copy of both the first changeset in the rebased and some other in the
set.

  $ hg graft 42ccdea3bb16 32af7686d403
  grafting 42ccdea3bb16 "B"
  grafting 32af7686d403 "D"
  $ hg rebase  -s 42ccdea3bb16 -d .
  rebasing 42ccdea3bb16 "B"
  note: rebase of 1:42ccdea3bb16 created no changes to commit
  rebasing 5fddd98957c8 "C"
  rebasing 32af7686d403 "D"
  note: rebase of 3:32af7686d403 created no changes to commit
  $ hg log -G
  o  10:5ae4c968c6ac C
  |
  @  9:08483444fef9 D
  |
  o  8:8877864f1edb B
  |
  | o  7:02de42196ebe H
  | |
  o |  6:eea13746799a G
  |\|
  | o  5:24b6387c8c8c F
  | |
  o |  4:9520eea781bc E
  |/
  o  0:cd010b8cd998 A
  
  $ hg log --hidden -G
  o  10:5ae4c968c6ac C
  |
  @  9:08483444fef9 D
  |
  o  8:8877864f1edb B
  |
  | o  7:02de42196ebe H
  | |
  o |  6:eea13746799a G
  |\|
  | o  5:24b6387c8c8c F
  | |
  o |  4:9520eea781bc E
  |/
  | x  3:32af7686d403 D
  | |
  | x  2:5fddd98957c8 C (rewritten using rebase as 10:5ae4c968c6ac)
  | |
  | x  1:42ccdea3bb16 B
  |/
  o  0:cd010b8cd998 A
  
  $ hg debugmutation 10
   *  5ae4c968c6aca831df823664e706c9d4aa34473d rebase by test at 1970-01-01T00:00:00 from:
      5fddd98957c8a54a4d436dfe1da9d87f21a1b97b

More complex case where part of the rebase set were already rebased

  $ hg rebase --rev 'desc(D)' --dest 'desc(H)'
  rebasing 08483444fef9 "D"
  $ hg debugmutation 11
   *  4596109a6a4328c398bde3a4a3b6737cfade3003 rebase by test at 1970-01-01T00:00:00 from:
      08483444fef91d6224f6655ee586a65d263ad34c
  $ hg log -G
  @  11:4596109a6a43 D
  |
  | o  10:5ae4c968c6ac C
  | |
  | x  9:08483444fef9 D (rewritten using rebase as 11:4596109a6a43)
  | |
  | o  8:8877864f1edb B
  | |
  o |  7:02de42196ebe H
  | |
  | o  6:eea13746799a G
  |/|
  o |  5:24b6387c8c8c F
  | |
  | o  4:9520eea781bc E
  |/
  o  0:cd010b8cd998 A
  
  $ hg rebase --source 'desc(B)' --dest 'tip' --config experimental.rebaseskipobsolete=True
  rebasing 8877864f1edb "B"
  note: not rebasing 08483444fef9 "D", already in destination as 4596109a6a43 "D"
  rebasing 5ae4c968c6ac "C"
  $ hg debugmutation 11::13
   *  4596109a6a4328c398bde3a4a3b6737cfade3003 rebase by test at 1970-01-01T00:00:00 from:
      08483444fef91d6224f6655ee586a65d263ad34c
   *  462a34d07e599b87ea08676a449373fe4e2e1347 rebase by test at 1970-01-01T00:00:00 from:
      8877864f1edb05d0e07dc4ba77b67a80a7b86672
   *  98f6af4ee9539e14da4465128f894c274900b6e5 rebase by test at 1970-01-01T00:00:00 from:
      5ae4c968c6aca831df823664e706c9d4aa34473d rebase by test at 1970-01-01T00:00:00 from:
      5fddd98957c8a54a4d436dfe1da9d87f21a1b97b
  $ hg log -G
  o  13:98f6af4ee953 C
  |
  o  12:462a34d07e59 B
  |
  @  11:4596109a6a43 D
  |
  o  7:02de42196ebe H
  |
  | o  6:eea13746799a G
  |/|
  o |  5:24b6387c8c8c F
  | |
  | o  4:9520eea781bc E
  |/
  o  0:cd010b8cd998 A
  
  $ hg log --style default --debug -r 4596109a6a4328c398bde3a4a3b6737cfade3003
  changeset:   11:4596109a6a4328c398bde3a4a3b6737cfade3003
  phase:       draft
  parent:      7:02de42196ebee42ef284b6780a87cdc96e8eaab6
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    a91006e3a02f1edf631f7018e6e5684cf27dd905
  user:        Nicolas Dumazet <nicdumz.commits@gmail.com>
  date:        Sat Apr 30 15:24:48 2011 +0200
  files+:      D
  extra:       branch=default
  extra:       rebase_source=08483444fef91d6224f6655ee586a65d263ad34c
  extra:       source=32af7686d403cf45b5d95f2d70cebea587ac806a
  description:
  D
  
  
  $ hg up -qr 'desc(G)'
  $ hg graft 4596109a6a4328c398bde3a4a3b6737cfade3003
  grafting 4596109a6a43 "D"
  $ hg up -qr 'desc(E)'
  $ hg rebase -s tip -d .
  rebasing 9e36056a46e3 "D"
  $ hg log --style default --debug -r tip
  changeset:   15:627d4614809036ba22b9e7cb31638ddc06ab99ab
  phase:       draft
  parent:      4:9520eea781bcca16c1e15acc0ba14335a0e8e5ba
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    648e8ede73ae3e497d093d3a4c8fcc2daa864f42
  user:        Nicolas Dumazet <nicdumz.commits@gmail.com>
  date:        Sat Apr 30 15:24:48 2011 +0200
  files+:      D
  extra:       branch=default
  extra:       intermediate-source=4596109a6a4328c398bde3a4a3b6737cfade3003
  extra:       rebase_source=9e36056a46e37c9776168c7375734eebc70e294f
  extra:       source=32af7686d403cf45b5d95f2d70cebea587ac806a
  description:
  D
  
  
Start rebase from a commit that is obsolete but not hidden only because it's
a working copy parent. We should be moved back to the starting commit as usual
even though it is hidden (until we're moved there).

  $ hg --hidden up -qr 'first(hidden())'
  $ hg rebase --rev 13 --dest 15
  rebasing 98f6af4ee953 "C"
  $ hg log -G
  o  16:294a2b93eb4d C
  |
  o  15:627d46148090 D
  |
  | o  12:462a34d07e59 B
  | |
  | o  11:4596109a6a43 D
  | |
  | o  7:02de42196ebe H
  | |
  +---o  6:eea13746799a G
  | |/
  | o  5:24b6387c8c8c F
  | |
  o |  4:9520eea781bc E
  |/
  | @  1:42ccdea3bb16 B
  |/
  o  0:cd010b8cd998 A
  

  $ cd ..

collapse rebase
---------------------------------

  $ hg clone base collapse
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd collapse
  $ hg rebase  -s 42ccdea3bb16 -d eea13746799a --collapse
  rebasing 42ccdea3bb16 "B"
  rebasing 5fddd98957c8 "C"
  rebasing 32af7686d403 "D"
  $ hg log -G
  o  8:4dc2197e807b Collapsed revision
  |
  | @  7:02de42196ebe H
  | |
  o |  6:eea13746799a G
  |\|
  | o  5:24b6387c8c8c F
  | |
  o |  4:9520eea781bc E
  |/
  o  0:cd010b8cd998 A
  
  $ hg log --hidden -G
  o  8:4dc2197e807b Collapsed revision
  |
  | @  7:02de42196ebe H
  | |
  o |  6:eea13746799a G
  |\|
  | o  5:24b6387c8c8c F
  | |
  o |  4:9520eea781bc E
  |/
  | x  3:32af7686d403 D (rewritten using rebase as 8:4dc2197e807b)
  | |
  | x  2:5fddd98957c8 C (rewritten using rebase as 8:4dc2197e807b)
  | |
  | x  1:42ccdea3bb16 B (rewritten using rebase as 8:4dc2197e807b)
  |/
  o  0:cd010b8cd998 A
  
  $ hg id --debug -r tip
  4dc2197e807bae9817f09905b50ab288be2dbbcf
  $ hg debugmutation 8
   *  4dc2197e807bae9817f09905b50ab288be2dbbcf rebase by test at 1970-01-01T00:00:00 from:
      |-  42ccdea3bb16d28e1848c95fe2e44c000f3f21b1
      |-  5fddd98957c8a54a4d436dfe1da9d87f21a1b97b
      '-  32af7686d403cf45b5d95f2d70cebea587ac806a

  $ cd ..

Rebase set has hidden descendants
---------------------------------

We rebase a changeset which has hidden descendants. Hidden changesets must not
be rebased.

  $ hg clone base hidden
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd hidden
  $ hg log -G
  @  7:02de42196ebe H
  |
  | o  6:eea13746799a G
  |/|
  o |  5:24b6387c8c8c F
  | |
  | o  4:9520eea781bc E
  |/
  | o  3:32af7686d403 D
  | |
  | o  2:5fddd98957c8 C
  | |
  | o  1:42ccdea3bb16 B
  |/
  o  0:cd010b8cd998 A
  
  $ hg rebase -s 5fddd98957c8 -d eea13746799a
  rebasing 5fddd98957c8 "C"
  rebasing 32af7686d403 "D"
  $ hg log -G
  o  9:cf44d2f5a9f4 D
  |
  o  8:e273c5e7d2d2 C
  |
  | @  7:02de42196ebe H
  | |
  o |  6:eea13746799a G
  |\|
  | o  5:24b6387c8c8c F
  | |
  o |  4:9520eea781bc E
  |/
  | o  1:42ccdea3bb16 B
  |/
  o  0:cd010b8cd998 A
  
  $ hg rebase -s 42ccdea3bb16 -d 02de42196ebe
  rebasing 42ccdea3bb16 "B"
  $ hg log -G
  o  10:7c6027df6a99 B
  |
  | o  9:cf44d2f5a9f4 D
  | |
  | o  8:e273c5e7d2d2 C
  | |
  @ |  7:02de42196ebe H
  | |
  | o  6:eea13746799a G
  |/|
  o |  5:24b6387c8c8c F
  | |
  | o  4:9520eea781bc E
  |/
  o  0:cd010b8cd998 A
  
  $ hg log --hidden -G
  o  10:7c6027df6a99 B
  |
  | o  9:cf44d2f5a9f4 D
  | |
  | o  8:e273c5e7d2d2 C
  | |
  @ |  7:02de42196ebe H
  | |
  | o  6:eea13746799a G
  |/|
  o |  5:24b6387c8c8c F
  | |
  | o  4:9520eea781bc E
  |/
  | x  3:32af7686d403 D (rewritten using rebase as 9:cf44d2f5a9f4)
  | |
  | x  2:5fddd98957c8 C (rewritten using rebase as 8:e273c5e7d2d2)
  | |
  | x  1:42ccdea3bb16 B (rewritten using rebase as 10:7c6027df6a99)
  |/
  o  0:cd010b8cd998 A
  
  $ hg debugmutation 8 9 10
   *  e273c5e7d2d29df783dce9f9eaa3ac4adc69c15d rebase by test at 1970-01-01T00:00:00 from:
      5fddd98957c8a54a4d436dfe1da9d87f21a1b97b
   *  cf44d2f5a9f4297a62be94cbdd3dff7c7dc54258 rebase by test at 1970-01-01T00:00:00 from:
      32af7686d403cf45b5d95f2d70cebea587ac806a
   *  7c6027df6a99d93f461868e5433f63bde20b6dfb rebase by test at 1970-01-01T00:00:00 from:
      42ccdea3bb16d28e1848c95fe2e44c000f3f21b1

Test that rewriting leaving instability behind is allowed
---------------------------------------------------------------------

  $ hg log -r 'children(8)'
  9:cf44d2f5a9f4 D (no-eol)
  $ hg rebase -r 8
  rebasing e273c5e7d2d2 "C"
  $ hg log -G
  o  11:0d8f238b634c C
  |
  o  10:7c6027df6a99 B
  |
  | o  9:cf44d2f5a9f4 D
  | |
  | x  8:e273c5e7d2d2 C (rewritten using rebase as 11:0d8f238b634c)
  | |
  @ |  7:02de42196ebe H
  | |
  | o  6:eea13746799a G
  |/|
  o |  5:24b6387c8c8c F
  | |
  | o  4:9520eea781bc E
  |/
  o  0:cd010b8cd998 A
  


Test multiple root handling
------------------------------------

  $ hg rebase --dest 4 --rev '7+11+9'
  rebasing 02de42196ebe "H"
  rebasing cf44d2f5a9f4 "D"
  rebasing 0d8f238b634c "C"
  $ hg log -G
  o  14:1e8370e38cca C
  |
  | o  13:102b4c1d889b D
  | |
  @ |  12:bfe264faf697 H
  |/
  | o  10:7c6027df6a99 B
  | |
  | x  7:02de42196ebe H (rewritten using rebase as 12:bfe264faf697)
  | |
  +---o  6:eea13746799a G
  | |/
  | o  5:24b6387c8c8c F
  | |
  o |  4:9520eea781bc E
  |/
  o  0:cd010b8cd998 A
  
  $ cd ..

Detach both parents

  $ hg init double-detach
  $ cd double-detach

  $ drawdag <<EOF
  >   F
  >  /|
  > C E
  > | |
  > B D G
  >  \|/
  >   A
  > EOF

  $ hg rebase -d "desc(G)" -r "desc(B) + desc(D) + desc(F)"
  rebasing 112478962961 "B"
  rebasing b18e25de2cf5 "D"
  rebasing f15c3adaf214 "F"
  abort: cannot rebase 6:f15c3adaf214 without moving at least one of its parents
  [255]

  $ cd ..

test on rebase dropping a merge

(setup)

  $ hg init dropmerge
  $ cd dropmerge
  $ hg unbundle "$TESTDIR/bundles/rebase.hg"
  adding changesets
  adding manifests
  adding file changes
  added 8 changesets with 7 changes to 7 files
  $ hg up 3
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg merge 7
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m 'M'
  $ echo I > I
  $ hg add I
  $ hg ci -m I
  $ hg log -G
  @  9:4bde274eefcf I
  |
  o    8:53a6a128b2b7 M
  |\
  | o  7:02de42196ebe H
  | |
  | | o  6:eea13746799a G
  | |/|
  | o |  5:24b6387c8c8c F
  | | |
  | | o  4:9520eea781bc E
  | |/
  o |  3:32af7686d403 D
  | |
  o |  2:5fddd98957c8 C
  | |
  o |  1:42ccdea3bb16 B
  |/
  o  0:cd010b8cd998 A
  
(actual test)

  $ hg rebase --dest 6 --rev '((desc(H) + desc(D))::) - desc(M)'
  rebasing 32af7686d403 "D"
  rebasing 02de42196ebe "H"
  rebasing 4bde274eefcf "I"
  $ hg log -G
  @  12:acd174b7ab39 I
  |
  o  11:6c11a6218c97 H
  |
  | o  10:b5313c85b22e D
  |/
  | o    8:53a6a128b2b7 M
  | |\
  | | x  7:02de42196ebe H (rewritten using rebase as 11:6c11a6218c97)
  | | |
  o---+  6:eea13746799a G
  | | |
  | | o  5:24b6387c8c8c F
  | | |
  o---+  4:9520eea781bc E
   / /
  x |  3:32af7686d403 D (rewritten using rebase as 10:b5313c85b22e)
  | |
  o |  2:5fddd98957c8 C
  | |
  o |  1:42ccdea3bb16 B
  |/
  o  0:cd010b8cd998 A
  

Test hidden changesets in the rebase set (issue4504)

  $ hg up --hidden 9
  3 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo J > J
  $ hg add J
  $ hg commit -m J
  $ hg --config extensions.amend= hide -q ".^"
  $ hg up -q --hidden "desc(J)"

  $ hg rebase --rev .~1::. --dest 'max(desc(D))' --traceback --config experimental.rebaseskipobsolete=off
  rebasing 4bde274eefcf "I"
  rebasing 06edfc82198f "J"
  $ hg log -G
  @  15:5ae8a643467b J
  |
  o  14:9ad579b4a5de I
  |
  | o  12:acd174b7ab39 I
  | |
  | o  11:6c11a6218c97 H
  | |
  o |  10:b5313c85b22e D
  |/
  | o    8:53a6a128b2b7 M
  | |\
  | | x  7:02de42196ebe H (rewritten using rebase as 11:6c11a6218c97)
  | | |
  o---+  6:eea13746799a G
  | | |
  | | o  5:24b6387c8c8c F
  | | |
  o---+  4:9520eea781bc E
   / /
  x |  3:32af7686d403 D (rewritten using rebase as 10:b5313c85b22e)
  | |
  o |  2:5fddd98957c8 C
  | |
  o |  1:42ccdea3bb16 B
  |/
  o  0:cd010b8cd998 A
  
  $ hg up 14 -C
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo "K" > K
  $ hg add K
  $ hg commit --amend -m "K"
  $ echo "L" > L
  $ hg add L
  $ hg commit -m "L"
  $ hg up '.^'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo "M" > M
  $ hg add M
  $ hg commit --amend -m "M"
  $ hg log -G
  @  18:bfaedf8eb73b M
  |
  | o  17:97219452e4bd L
  | |
  | x  16:fc37a630c901 K (rewritten using amend as 18:bfaedf8eb73b)
  |/
  | o  15:5ae8a643467b J
  | |
  | x  14:9ad579b4a5de I (rewritten using amend as 16:fc37a630c901)
  |/
  | o  12:acd174b7ab39 I
  | |
  | o  11:6c11a6218c97 H
  | |
  o |  10:b5313c85b22e D
  |/
  | o    8:53a6a128b2b7 M
  | |\
  | | x  7:02de42196ebe H (rewritten using rebase as 11:6c11a6218c97)
  | | |
  o---+  6:eea13746799a G
  | | |
  | | o  5:24b6387c8c8c F
  | | |
  o---+  4:9520eea781bc E
   / /
  x |  3:32af7686d403 D (rewritten using rebase as 10:b5313c85b22e)
  | |
  o |  2:5fddd98957c8 C
  | |
  o |  1:42ccdea3bb16 B
  |/
  o  0:cd010b8cd998 A
  
  $ hg rebase -s 14 -d 17 --config experimental.rebaseskipobsolete=True
  note: not rebasing 9ad579b4a5de "I", already in destination as fc37a630c901 "K"
  rebasing 5ae8a643467b "J"

  $ cd ..

Skip obsolete changeset even with multiple hops
-----------------------------------------------

setup

  $ hg init obsskip
  $ cd obsskip
  $ cat << EOF >> .hg/hgrc
  > [experimental]
  > rebaseskipobsolete = True
  > [extensions]
  > strip =
  > EOF
  $ echo A > A
  $ hg add A
  $ hg commit -m A
  $ echo B > B
  $ hg add B
  $ hg commit -m B0
  $ hg commit --amend -m B1
  $ hg commit --amend -m B2
  $ hg up --hidden 'desc(B0)'
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo C > C
  $ hg add C
  $ hg commit -m C
  $ hg log -G
  @  4:212cb178bcbb C
  |
  | o  3:261e70097290 B2
  | |
  x |  1:a8b11f55fb19 B0 (rewritten using rewrite as 3:261e70097290)
  |/
  o  0:4a2df7238c3b A
  

Rebase finds its way in a chain of marker

  $ hg rebase -d 'desc(B2)'
  note: not rebasing a8b11f55fb19 "B0", already in destination as 261e70097290 "B2"
  rebasing 212cb178bcbb "C"

Even when the chain include missing node

  $ hg up --hidden 'desc(B0)'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo D > D
  $ hg add D
  $ hg commit -m D
  $ hg --hidden debugstrip -r 'desc(B1)'
  saved backup bundle to $TESTTMP/obsskip/.hg/strip-backup/86f6414ccda7-b1c452ee-backup.hg
  $ hg log -G
  @  5:1a79b7535141 D
  |
  | o  4:ff2c4d47b71d C
  | |
  | o  2:261e70097290 B2
  | |
  x |  1:a8b11f55fb19 B0 (rewritten using rewrite as 2:261e70097290)
  |/
  o  0:4a2df7238c3b A
  

  $ hg rebase -d 'desc(B2)'
  note: not rebasing a8b11f55fb19 "B0", already in destination as 261e70097290 "B2"
  rebasing 1a79b7535141 "D"
  $ hg up 4
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo "O" > O
  $ hg add O
  $ hg commit -m O
  $ echo "P" > P
  $ hg add P
  $ hg commit -m P
  $ hg log -G
  @  8:8d47583e023f P
  |
  o  7:360bbaa7d3ce O
  |
  | o  6:9c48361117de D
  | |
  o |  4:ff2c4d47b71d C
  |/
  o  2:261e70097290 B2
  |
  o  0:4a2df7238c3b A
  
  $ hg rebase -d 6 -r "4+8"
  rebasing ff2c4d47b71d "C"
  rebasing 8d47583e023f "P"
  $ hg hide 7 --config extensions.amend=
  hiding commit 360bbaa7d3ce "O"
  1 changeset hidden

If all the changeset to be rebased are obsolete and present in the destination, we
should display a friendly error message

  $ hg log -G
  @  10:121d9e3bc4c6 P
  |
  o  9:4be60e099a77 C
  |
  o  6:9c48361117de D
  |
  o  2:261e70097290 B2
  |
  o  0:4a2df7238c3b A
  

Rebases can create divergence

  $ hg log -G
  @  10:121d9e3bc4c6 P
  |
  o  9:4be60e099a77 C
  |
  o  6:9c48361117de D
  |
  o  2:261e70097290 B2
  |
  o  0:4a2df7238c3b A
  

  $ hg up 9
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo "john" > doe
  $ hg add doe
  $ hg commit -m "john doe"
  $ hg up 10
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo "foo" > bar
  $ hg add bar
  $ hg commit --amend -m "P-amended"
  $ hg up 10 --hidden
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo "bar" > foo
  $ hg add foo
  $ hg commit -m "bar foo"
  $ hg log -G
  @  13:73568ab6879d bar foo
  |
  | o  12:e75f87cd16c5 P-amended
  | |
  | | o  11:3eb461388009 john doe
  | |/
  x |  10:121d9e3bc4c6 P (rewritten using amend as 12:e75f87cd16c5)
  |/
  o  9:4be60e099a77 C
  |
  o  6:9c48361117de D
  |
  o  2:261e70097290 B2
  |
  o  0:4a2df7238c3b A
  
  $ hg rebase -s 10 -d 11
  rebasing 121d9e3bc4c6 "P"
  rebasing 73568ab6879d "bar foo"
  $ hg log -G
  @  15:61bd55f69bc4 bar foo
  |
  o  14:5f53594f6882 P
  |
  | o  12:e75f87cd16c5 P-amended
  | |
  o |  11:3eb461388009 john doe
  |/
  o  9:4be60e099a77 C
  |
  o  6:9c48361117de D
  |
  o  2:261e70097290 B2
  |
  o  0:4a2df7238c3b A
  
rebase --continue + skipped rev because their successors are in destination
we make a change in trunk and work on conflicting changes to make rebase abort.

  $ hg log -G -r 15::
  @  15:61bd55f69bc4 bar foo
  |
  ~

Create a change in trunk
  $ printf "a" > willconflict
  $ hg add willconflict
  $ hg commit -m "willconflict first version"

Create the changes that we will rebase
  $ hg update -C 15 -q
  $ printf "b" > willconflict
  $ hg add willconflict
  $ hg commit -m "willconflict second version"
  $ printf "dummy" > K
  $ hg add K
  $ hg commit -m "dummy change 1"
  $ printf "dummy" > L
  $ hg add L
  $ hg commit -m "dummy change 2"
  $ hg rebase -r 18 -d 16
  rebasing cab092d71c4b "dummy change 1"

  $ hg log -G -r 15::
  o  20:59c6f3a91215 dummy change 1
  |
  | @  19:ae4ed1351416 dummy change 2
  | |
  | x  18:cab092d71c4b dummy change 1 (rewritten using rebase as 20:59c6f3a91215)
  | |
  | o  17:b82fb57ea638 willconflict second version
  | |
  o |  16:357ddf1602d5 willconflict first version
  |/
  o  15:61bd55f69bc4 bar foo
  |
  ~
  $ hg rebase -r ".^^ + .^ + ." -d 20
  rebasing b82fb57ea638 "willconflict second version"
  merging willconflict
  warning: 1 conflicts while merging willconflict! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

  $ hg resolve --mark willconflict
  (no more unresolved files)
  continue: hg rebase --continue
  $ hg rebase --continue
  rebasing b82fb57ea638 "willconflict second version"
  note: not rebasing cab092d71c4b "dummy change 1", already in destination as 59c6f3a91215 "dummy change 1"
  rebasing ae4ed1351416 "dummy change 2"
  $ cd ..

Divergence cases due to obsolete changesets
-------------------------------------------

We should ignore branches with unstable changesets when they are based on an
obsolete changeset which successor is in rebase set.

  $ hg init divergence
  $ cd divergence
  $ cat >> .hg/hgrc << EOF
  > [templates]
  > instabilities = '{rev}:{node|short} {desc|firstline}{if(instabilities," ({instabilities})")}\n'
  > EOF

  $ drawdag <<EOF
  >   e   f
  >   |   |
  >   d2  d # replace: d -> d2
  >    \ /
  >     c
  >     |
  >   x b
  >    \|
  >     a
  > EOF
  $ hg log -G -r $a::
  o  7:493e1ea05b71 e
  |
  | o  6:1143e9adc121 f
  | |
  o |  5:447acf26a46a d2
  | |
  | x  4:76be324c128b d (rewritten using replace as 5:447acf26a46a)
  |/
  o  3:a82ac2b38757 c
  |
  | o  2:630d7c95eff7 x
  | |
  o |  1:488e1b7e7341 b
  |/
  o  0:b173517d0057 a
  

Changeset d and its descendants are excluded to avoid divergence of d, which
would occur because the successor of d (d2) is also in rebaseset. As a
consequence f (descendant of d) is left behind.

  $ hg rebase -b $e -d $x
  rebasing 488e1b7e7341 "b"
  rebasing a82ac2b38757 "c"
  note: not rebasing 76be324c128b "d" and its descendants as this would cause divergence
  rebasing 447acf26a46a "d2"
  rebasing 493e1ea05b71 "e"
  $ hg log -G -r $a::
  o  11:1ce56955f155 e
  |
  o  10:885d062b1232 d2
  |
  o  9:d008e6b4d3fd c
  |
  o  8:67e8f4a16c49 b
  |
  | o  6:1143e9adc121 f
  | |
  | x  4:76be324c128b d (rewritten using rebase-copy as 10:885d062b1232)
  | |
  | x  3:a82ac2b38757 c (rewritten using rebase as 9:d008e6b4d3fd)
  | |
  o |  2:630d7c95eff7 x
  | |
  | x  1:488e1b7e7341 b (rewritten using rebase as 8:67e8f4a16c49)
  |/
  o  0:b173517d0057 a
  
  $ hg debugstrip --no-backup -q -r 8:

If the rebase set has an obsolete (d) with a successor (d2) outside the rebase
set and none in destination, then divergence is allowed.

  $ hg unhide -r $d2+$e --config extensions.amend=
  $ hg rebase -r $c::$f -d $x
  rebasing a82ac2b38757 "c"
  rebasing 76be324c128b "d"
  rebasing 1143e9adc121 "f"
  $ hg log -G -r $a::
  o  10:e1744ea07510 f
  |
  o  9:e2b36ea9a0a0 d
  |
  o  8:6a0376de376e c
  |
  | o  7:493e1ea05b71 e
  | |
  | o  5:447acf26a46a d2
  | |
  | x  3:a82ac2b38757 c (rewritten using rebase as 8:6a0376de376e)
  | |
  o |  2:630d7c95eff7 x
  | |
  | o  1:488e1b7e7341 b
  |/
  o  0:b173517d0057 a
  
  $ hg debugstrip --no-backup -q -r 8:

(Not skipping obsoletes means that divergence is allowed.)

  $ hg unhide -r $f --config extensions.amend=
  $ hg rebase --config experimental.rebaseskipobsolete=false -r $c::$f -d $x
  rebasing a82ac2b38757 "c"
  rebasing 76be324c128b "d"
  rebasing 1143e9adc121 "f"

  $ hg debugstrip --no-backup -q -r 0:

Similar test on a more complex graph

  $ drawdag <<EOF
  >       g
  >       |
  >   f   e
  >   |   |
  >   e2  d # replace: e -> e2
  >    \ /
  >     c
  >     |
  >   x b
  >    \|
  >     a
  > EOF
  $ hg log -G -r $a:
  o  8:12df856f4e8e f
  |
  | o  7:2876ce66c6eb g
  | |
  o |  6:87682c149ad7 e2
  | |
  | x  5:e36fae928aec e (rewritten using replace as 6:87682c149ad7)
  | |
  | o  4:76be324c128b d
  |/
  o  3:a82ac2b38757 c
  |
  | o  2:630d7c95eff7 x
  | |
  o |  1:488e1b7e7341 b
  |/
  o  0:b173517d0057 a
  
  $ hg rebase -b $f -d $x
  rebasing 488e1b7e7341 "b"
  rebasing a82ac2b38757 "c"
  rebasing 76be324c128b "d"
  note: not rebasing e36fae928aec "e" and its descendants as this would cause divergence
  rebasing 87682c149ad7 "e2"
  rebasing 12df856f4e8e "f"
  $ hg log -G -r $a:
  o  13:aa3f1f628d29 f
  |
  o  12:2963fc7a5743 e2
  |
  | o  11:a1707a5b7c2c d
  |/
  o  10:d008e6b4d3fd c
  |
  o  9:67e8f4a16c49 b
  |
  | o  7:2876ce66c6eb g
  | |
  | x  5:e36fae928aec e (rewritten using rebase-copy as 12:2963fc7a5743)
  | |
  | x  4:76be324c128b d (rewritten using rebase as 11:a1707a5b7c2c)
  | |
  | x  3:a82ac2b38757 c (rewritten using rebase as 10:d008e6b4d3fd)
  | |
  o |  2:630d7c95eff7 x
  | |
  | x  1:488e1b7e7341 b (rewritten using rebase as 9:67e8f4a16c49)
  |/
  o  0:b173517d0057 a
  

  $ cd ..

Rebase merge where successor of one parent is equal to destination (issue5198)

  $ hg init p1-succ-is-dest
  $ cd p1-succ-is-dest

  $ drawdag <<EOF
  >   F
  >  /|
  > E D B # replace: D -> B
  >  \|/
  >   A
  > EOF

  $ hg rebase -d $B -s "desc(D)"
  note: not rebasing b18e25de2cf5 "D", already in destination as 112478962961 "B"
  rebasing 66f1a38021c9 "F"
  $ hg log -G
  o    5:50e9d60b99c6 F
  |\
  | o  3:112478962961 B
  | |
  o |  2:7fb047a69f22 E
  |/
  o  0:426bada5c675 A
  
  $ cd ..

Rebase merge where successor of other parent is equal to destination

  $ hg init p2-succ-is-dest
  $ cd p2-succ-is-dest

  $ drawdag <<EOF
  >   F
  >  /|
  > E D B # replace: E -> B
  >  \|/
  >   A
  > EOF

  $ hg rebase -d "desc(B)" -s "desc(E)"
  note: not rebasing 7fb047a69f22 "E", already in destination as 112478962961 "B"
  rebasing 66f1a38021c9 "F"
  $ hg log -G
  o    5:aae1787dacee F
  |\
  | o  3:112478962961 B
  | |
  o |  1:b18e25de2cf5 D
  |/
  o  0:426bada5c675 A
  
  $ cd ..

Rebase merge where successor of one parent is ancestor of destination

  $ hg init p1-succ-in-dest
  $ cd p1-succ-in-dest

  $ drawdag <<EOF
  >   F C
  >  /| |
  > E D B # replace: D -> B
  >  \|/
  >   A
  > EOF

  $ hg rebase -d "desc(C)" -s "desc(D)"
  note: not rebasing b18e25de2cf5 "D", already in destination as 112478962961 "B"
  rebasing 66f1a38021c9 "F"

  $ hg log -G
  o    6:0913febf6439 F
  |\
  | o  5:26805aba1e60 C
  | |
  | o  3:112478962961 B
  | |
  o |  2:7fb047a69f22 E
  |/
  o  0:426bada5c675 A
  
  $ cd ..

Rebase merge where successor of other parent is ancestor of destination

  $ hg init p2-succ-in-dest
  $ cd p2-succ-in-dest

  $ drawdag <<EOF
  >   F C
  >  /| |
  > E D B # replace: E -> B
  >  \|/
  >   A
  > EOF

  $ hg rebase -d "desc(C)" -s "desc(E)"
  note: not rebasing 7fb047a69f22 "E", already in destination as 112478962961 "B"
  rebasing 66f1a38021c9 "F"
  $ hg log -G
  o    6:c6ab0cc6d220 F
  |\
  | o  5:26805aba1e60 C
  | |
  | o  3:112478962961 B
  | |
  o |  1:b18e25de2cf5 D
  |/
  o  0:426bada5c675 A
  
  $ cd ..

Rebase merge where successor of one parent is ancestor of destination

  $ hg init p1-succ-in-dest-b
  $ cd p1-succ-in-dest-b

  $ drawdag <<EOF
  >   F C
  >  /| |
  > E D B # replace: E -> B
  >  \|/
  >   A
  > EOF

  $ hg rebase -d "desc(C)" -b "desc(F)"
  note: not rebasing 7fb047a69f22 "E", already in destination as 112478962961 "B"
  rebasing b18e25de2cf5 "D"
  rebasing 66f1a38021c9 "F"
  note: rebase of 4:66f1a38021c9 created no changes to commit
  $ hg log -G
  o  6:8f47515dda15 D
  |
  o  5:26805aba1e60 C
  |
  o  3:112478962961 B
  |
  o  0:426bada5c675 A
  
  $ cd ..

Rebase merge where successor of other parent is ancestor of destination

  $ hg init p2-succ-in-dest-b
  $ cd p2-succ-in-dest-b

  $ drawdag <<EOF
  >   F C
  >  /| |
  > E D B # replace: D -> B
  >  \|/
  >   A
  > EOF

  $ hg rebase -d "desc(C)" -b "desc(F)"
  rebasing 7fb047a69f22 "E"
  note: not rebasing b18e25de2cf5 "D", already in destination as 112478962961 "B"
  rebasing 66f1a38021c9 "F"
  note: rebase of 4:66f1a38021c9 created no changes to commit

  $ hg log -G
  o  6:533690786a86 E
  |
  o  5:26805aba1e60 C
  |
  o  3:112478962961 B
  |
  o  0:426bada5c675 A
  
  $ cd ..

Rebase merge where both parents have successors in destination

  $ hg init p12-succ-in-dest
  $ cd p12-succ-in-dest
  $ drawdag <<'EOS'
  >   E   F
  >  /|  /|  # replace: A -> C
  > A B C D  # replace: B -> D
  > | |
  > X Y
  > EOS
  $ hg rebase -r "desc(A)+desc(B)+desc(E)" -d "desc(F)"
  note: not rebasing a3d17304151f "A", already in destination as 96cc3511f894 "C"
  note: not rebasing b23a2cc00842 "B", already in destination as 058c1e1fb10a "D"
  rebasing dac5d11c5a7d "E"
  abort: rebasing 6:dac5d11c5a7d will include unwanted changes from 1:59c792af609c, 3:b23a2cc00842 or 0:ba2b7fa7166d, 2:a3d17304151f
  [255]
  $ cd ..

Rebase a non-clean merge. One parent has successor in destination, the other
parent moves as requested.

  $ hg init p1-succ-p2-move
  $ cd p1-succ-p2-move
  $ drawdag <<'EOS'
  >   D Z
  >  /| | # replace: A -> C
  > A B C # D/D = D
  > EOS
  $ hg rebase -r "desc(A)+desc(B)+desc(D)" -d "desc(Z)"
  note: not rebasing 426bada5c675 "A", already in destination as 96cc3511f894 "C"
  rebasing fc2b737bb2e5 "B"
  rebasing b8ed089c80ad "D"

  $ hg log -G
  o  6:e4f78693cc88 D
  |
  o  5:76840d832e98 B
  |
  o  4:50e41c1f3950 Z
  |
  o  2:96cc3511f894 C
  
  $ hg files -r tip
  B
  C
  D
  Z

  $ cd ..

  $ hg init p1-move-p2-succ
  $ cd p1-move-p2-succ
  $ drawdag <<'EOS'
  >   D Z
  >  /| |  # replace: B -> C
  > A B C  # D/D = D
  > EOS
  $ hg rebase -r "desc(B)+desc(A)+desc(D)" -d "desc(Z)"
  rebasing 426bada5c675 "A"
  note: not rebasing fc2b737bb2e5 "B", already in destination as 96cc3511f894 "C"
  rebasing b8ed089c80ad "D"

  $ hg log -G
  o  6:1b355ed94d82 D
  |
  o  5:a81a74d764a6 A
  |
  o  4:50e41c1f3950 Z
  |
  o  2:96cc3511f894 C
  
  $ hg files -r tip
  A
  C
  D
  Z

  $ cd ..

Test that bookmark is moved and working dir is updated when all changesets have
equivalents in destination
  $ hg init rbsrepo && cd rbsrepo
  $ echo "[experimental]" > .hg/hgrc
  $ echo "evolution=true" >> .hg/hgrc
  $ echo "rebaseskipobsolete=on" >> .hg/hgrc
  $ echo root > root && hg ci -Am root
  adding root
  $ echo a > a && hg ci -Am a
  adding a
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo b > b && hg ci -Am b
  adding b
  $ hg rebase -r 2 -d 1
  rebasing 1e9a3c00cbe9 "b"
  $ hg log -r .  # working dir is at rev 3 (successor of 2)
  3:be1832deae9a b (no-eol)
  $ hg book -r 2 mybook --hidden  # rev 2 has a bookmark on it now
  $ hg up 2 && hg log -r .  # working dir is at rev 2 again
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  2:1e9a3c00cbe9 b (rewritten using rebase as 3:be1832deae9a) (no-eol)
  $ hg rebase -r 2 -d 3 --config experimental.evolution.track-operation=1
  note: not rebasing 1e9a3c00cbe9 "b" (mybook), already in destination as be1832deae9a "b"
Check that working directory and bookmark was updated to rev 3 although rev 2
was skipped
  $ hg log -r .
  3:be1832deae9a b (no-eol)
  $ hg bookmarks
     mybook                    3:be1832deae9a
  $ hg debugobsolete --rev tip
  1e9a3c00cbe90d236ac05ef61efcc5e40b7412bc be1832deae9ac531caa7438b8dcf6055a122cd8e 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'rebase', 'user': 'test'}

Obsoleted working parent and bookmark could be moved if an ancestor of working
parent gets moved:

  $ hg init $TESTTMP/ancestor-wd-move
  $ cd $TESTTMP/ancestor-wd-move
  $ drawdag <<'EOS'
  >  E D1  # rebase: D1 -> D2
  >  | |
  >  | C
  > D2 |
  >  | B
  >  |/
  >  A
  > EOS
  $ hg update "desc(D1)" -q --hidden
  $ hg bookmark book -i
  $ hg rebase -r "desc(B)+desc(D1)" -d "desc(E)"
  rebasing 112478962961 "B"
  note: not rebasing 15ecf15e0114 "D1" (book), already in destination as 0807738e0be9 "D2"
  $ hg log -G -T '{desc} {bookmarks}'
  @  B book
  |
  o  E
  |
  o  D2
  |
  | o  C
  | |
  | x  B
  |/
  o  A
  
Rebasing a merge with one of its parent having a hidden successor

  $ hg init $TESTTMP/merge-p1-hidden-successor
  $ cd $TESTTMP/merge-p1-hidden-successor

  $ drawdag <<'EOS'
  >  E
  >  |
  > B3 B2 # amend: B1 -> B2 -> B3
  >  |/   # B2 is hidden
  >  |  D
  >  |  |\
  >  | B1 C
  >  |/
  >  A
  > EOS

  $ hg rebase -r $D -d $E
  rebasing 9e62094e4d94 "D"

  $ hg log -G
  o    7:a699d059adcf D
  |\
  | o  6:ecc93090a95c E
  | |
  | o  5:0dc878468a23 B3
  | |
  o |  1:96cc3511f894 C
   /
  o  0:426bada5c675 A
  
For some reasons (--hidden, rebaseskipobsolete=0, directaccess, etc.),
rebasestate may contain hidden hashes. "rebase --abort" should work regardless.

  $ hg init $TESTTMP/hidden-state1
  $ cd $TESTTMP/hidden-state1
  $ setconfig experimental.rebaseskipobsolete=0

  $ drawdag <<'EOS'
  >    C
  >    |
  >  D B
  >  |/  # B/D=B
  >  A
  > EOS

  $ hg hide -q $B --config extensions.amend=
  $ hg update -q $C --hidden
  $ hg rebase -s $B -d $D
  rebasing 2ec65233581b "B"
  merging D
  warning: 1 conflicts while merging D! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

  $ cp -R . $TESTTMP/hidden-state2

  $ hg log -G
  @  2:b18e25de2cf5 D
  |
  | @  1:2ec65233581b B
  |/
  o  0:426bada5c675 A
  
  $ hg summary
  parent: 2:b18e25de2cf5 
   D
  parent: 1:2ec65233581b 
   B
  commit: 2 modified, 1 unknown, 1 unresolved (merge)
  phases: 3 draft
  rebase: 0 rebased, 2 remaining (rebase --continue)

  $ hg rebase --abort
  rebase aborted

Also test --continue for the above case

  $ cd $TESTTMP/hidden-state2
  $ hg resolve -m
  (no more unresolved files)
  continue: hg rebase --continue
  $ hg rebase --continue
  rebasing 2ec65233581b "B"
  rebasing 7829726be4dc "C"
  $ hg log -G
  @  5:1964d5d5b547 C
  |
  o  4:68deb90c12a2 B
  |
  o  2:b18e25de2cf5 D
  |
  o  0:426bada5c675 A
  
