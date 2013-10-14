==========================
Test rebase with obsolete
==========================

Enable obsolete

  $ cat > ${TESTTMP}/obs.py << EOF
  > import mercurial.obsolete
  > mercurial.obsolete._enabled = True
  > EOF
  $ cat >> $HGRCPATH << EOF
  > [ui]
  > logtemplate= {rev}:{node|short} {desc|firstline}
  > [phases]
  > publish=False
  > [extensions]'
  > rebase=
  > 
  > obs=${TESTTMP}/obs.py
  > EOF

Setup rebase canonical repo

  $ hg init base
  $ cd base
  $ hg unbundle "$TESTDIR/bundles/rebase.hg"
  adding changesets
  adding manifests
  adding file changes
  added 8 changesets with 7 changes to 7 files (+2 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)
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
  | x  3:32af7686d403 D
  | |
  | x  2:5fddd98957c8 C
  | |
  | x  1:42ccdea3bb16 B
  |/
  o  0:cd010b8cd998 A
  
  $ hg debugobsolete
  42ccdea3bb16d28e1848c95fe2e44c000f3f21b1 e4e5be0395b2cbd471ed22a26b1b6a1a0658a794 0 {'date': '*', 'user': 'test'} (glob)
  5fddd98957c8a54a4d436dfe1da9d87f21a1b97b 2327fea05063f39961b14cb69435a9898dc9a245 0 {'date': '*', 'user': 'test'} (glob)
  32af7686d403cf45b5d95f2d70cebea587ac806a 8eeb3c33ad33d452c89e5dcf611c347f978fb42b 0 {'date': '*', 'user': 'test'} (glob)


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
  grafting revision 1
  grafting revision 3
  $ hg rebase  -s 42ccdea3bb16 -d .
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
  | x  2:5fddd98957c8 C
  | |
  | x  1:42ccdea3bb16 B
  |/
  o  0:cd010b8cd998 A
  
  $ hg debugobsolete
  42ccdea3bb16d28e1848c95fe2e44c000f3f21b1 0 {'date': '*', 'user': 'test'} (glob)
  5fddd98957c8a54a4d436dfe1da9d87f21a1b97b 5ae4c968c6aca831df823664e706c9d4aa34473d 0 {'date': '*', 'user': 'test'} (glob)
  32af7686d403cf45b5d95f2d70cebea587ac806a 0 {'date': '*', 'user': 'test'} (glob)


More complex case were part of the rebase set were already rebased

  $ hg rebase --rev 'desc(D)' --dest 'desc(H)'
  $ hg debugobsolete
  42ccdea3bb16d28e1848c95fe2e44c000f3f21b1 0 {'date': '*', 'user': 'test'} (glob)
  5fddd98957c8a54a4d436dfe1da9d87f21a1b97b 5ae4c968c6aca831df823664e706c9d4aa34473d 0 {'date': '*', 'user': 'test'} (glob)
  32af7686d403cf45b5d95f2d70cebea587ac806a 0 {'date': '*', 'user': 'test'} (glob)
  08483444fef91d6224f6655ee586a65d263ad34c 4596109a6a4328c398bde3a4a3b6737cfade3003 0 {'date': '* *', 'user': 'test'} (glob)
  $ hg log -G
  @  11:4596109a6a43 D
  |
  | o  10:5ae4c968c6ac C
  | |
  | x  9:08483444fef9 D
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
  
  $ hg rebase --source 'desc(B)' --dest 'tip'
  $ hg debugobsolete
  42ccdea3bb16d28e1848c95fe2e44c000f3f21b1 0 {'date': '* *', 'user': 'test'} (glob)
  5fddd98957c8a54a4d436dfe1da9d87f21a1b97b 5ae4c968c6aca831df823664e706c9d4aa34473d 0 {'date': '* *', 'user': 'test'} (glob)
  32af7686d403cf45b5d95f2d70cebea587ac806a 0 {'date': '* *', 'user': 'test'} (glob)
  08483444fef91d6224f6655ee586a65d263ad34c 4596109a6a4328c398bde3a4a3b6737cfade3003 0 {'date': '* *', 'user': 'test'} (glob)
  8877864f1edb05d0e07dc4ba77b67a80a7b86672 462a34d07e599b87ea08676a449373fe4e2e1347 0 {'date': '* *', 'user': 'test'} (glob)
  08483444fef91d6224f6655ee586a65d263ad34c 0 {'date': '* *', 'user': 'test'} (glob)
  5ae4c968c6aca831df823664e706c9d4aa34473d 98f6af4ee9539e14da4465128f894c274900b6e5 0 {'date': '* *', 'user': 'test'} (glob)
  $ hg log --rev 'divergent()'
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
  manifest:    11:a91006e3a02f1edf631f7018e6e5684cf27dd905
  user:        Nicolas Dumazet <nicdumz.commits@gmail.com>
  date:        Sat Apr 30 15:24:48 2011 +0200
  files+:      D
  extra:       branch=default
  extra:       rebase_source=08483444fef91d6224f6655ee586a65d263ad34c
  extra:       source=32af7686d403cf45b5d95f2d70cebea587ac806a
  description:
  D
  
  
  $ cd ..

collapse rebase
---------------------------------

  $ hg clone base collapse
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd collapse
  $ hg rebase  -s 42ccdea3bb16 -d eea13746799a --collapse
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
  | x  3:32af7686d403 D
  | |
  | x  2:5fddd98957c8 C
  | |
  | x  1:42ccdea3bb16 B
  |/
  o  0:cd010b8cd998 A
  
  $ hg id --debug -r tip
  4dc2197e807bae9817f09905b50ab288be2dbbcf tip
  $ hg debugobsolete
  42ccdea3bb16d28e1848c95fe2e44c000f3f21b1 4dc2197e807bae9817f09905b50ab288be2dbbcf 0 {'date': '*', 'user': 'test'} (glob)
  5fddd98957c8a54a4d436dfe1da9d87f21a1b97b 4dc2197e807bae9817f09905b50ab288be2dbbcf 0 {'date': '*', 'user': 'test'} (glob)
  32af7686d403cf45b5d95f2d70cebea587ac806a 4dc2197e807bae9817f09905b50ab288be2dbbcf 0 {'date': '*', 'user': 'test'} (glob)

  $ cd ..

Rebase set has hidden descendants
---------------------------------

We rebase a changeset which has a hidden changeset. The hidden changeset must
not be rebased.

  $ hg clone base hidden
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd hidden
  $ hg rebase -s 5fddd98957c8 -d eea13746799a
  $ hg rebase -s 42ccdea3bb16 -d 02de42196ebe
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
  | x  3:32af7686d403 D
  | |
  | x  2:5fddd98957c8 C
  | |
  | x  1:42ccdea3bb16 B
  |/
  o  0:cd010b8cd998 A
  
  $ hg debugobsolete
  5fddd98957c8a54a4d436dfe1da9d87f21a1b97b e273c5e7d2d29df783dce9f9eaa3ac4adc69c15d 0 {'date': '*', 'user': 'test'} (glob)
  32af7686d403cf45b5d95f2d70cebea587ac806a cf44d2f5a9f4297a62be94cbdd3dff7c7dc54258 0 {'date': '*', 'user': 'test'} (glob)
  42ccdea3bb16d28e1848c95fe2e44c000f3f21b1 7c6027df6a99d93f461868e5433f63bde20b6dfb 0 {'date': '*', 'user': 'test'} (glob)

Test that rewriting leaving instability behind is allowed
---------------------------------------------------------------------

  $ hg log -r 'children(8)'
  9:cf44d2f5a9f4 D (no-eol)
  $ hg rebase -r 8
  $ hg log -G
  o  11:0d8f238b634c C
  |
  o  10:7c6027df6a99 B
  |
  | o  9:cf44d2f5a9f4 D
  | |
  | x  8:e273c5e7d2d2 C
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
  $ hg log -G
  o  14:1e8370e38cca C
  |
  | o  13:102b4c1d889b D
  | |
  @ |  12:bfe264faf697 H
  |/
  | o  10:7c6027df6a99 B
  | |
  | x  7:02de42196ebe H
  | |
  +---o  6:eea13746799a G
  | |/
  | o  5:24b6387c8c8c F
  | |
  o |  4:9520eea781bc E
  |/
  o  0:cd010b8cd998 A
  
  $ cd ..

test on rebase dropping a merge

(setup)

  $ hg init dropmerge
  $ cd dropmerge
  $ hg unbundle "$TESTDIR/bundles/rebase.hg"
  adding changesets
  adding manifests
  adding file changes
  added 8 changesets with 7 changes to 7 files (+2 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)
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
  $ hg log -G
  @  12:acd174b7ab39 I
  |
  o  11:6c11a6218c97 H
  |
  | o  10:b5313c85b22e D
  |/
  | o    8:53a6a128b2b7 M
  | |\
  | | x  7:02de42196ebe H
  | | |
  o---+  6:eea13746799a G
  | | |
  | | o  5:24b6387c8c8c F
  | | |
  o---+  4:9520eea781bc E
   / /
  x |  3:32af7686d403 D
  | |
  o |  2:5fddd98957c8 C
  | |
  o |  1:42ccdea3bb16 B
  |/
  o  0:cd010b8cd998 A
  
