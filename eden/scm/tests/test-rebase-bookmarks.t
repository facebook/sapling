#chg-compatible

  $ setconfig extensions.treemanifest=!
TODO: Make this test compatibile with obsstore enabled.
  $ setconfig experimental.evolution=
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > rebase=
  > 
  > [phases]
  > publish=False
  > EOF

Create a repo with several bookmarks
  $ hg init a
  $ cd a

  $ echo a > a
  $ hg ci -Am A
  adding a

  $ echo b > b
  $ hg ci -Am B
  adding b
  $ hg book 'X'
  $ hg book 'Y'

  $ echo c > c
  $ hg ci -Am C
  adding c
  $ hg book 'Z'

  $ hg up -q 0

  $ echo d > d
  $ hg ci -Am D
  adding d

  $ hg book W

  $ tglog
  @  3: 41acb9dca9eb 'D' W
  |
  | o  2: 49cb3485fa0c 'C' Y Z
  | |
  | o  1: 6c81ed0049f8 'B' X
  |/
  o  0: 1994f17a630e 'A'
  

Move only rebased bookmarks

  $ cd ..
  $ hg clone -q a a1

  $ cd a1
  $ hg up -q Z

Test deleting divergent bookmarks from dest (issue3685)

  $ hg book -r 3 Z@diverge

... and also test that bookmarks not on dest or not being moved aren't deleted

  $ hg book -r 3 X@diverge
  $ hg book -r 0 Y@diverge

  $ tglog
  o  3: 41acb9dca9eb 'D' W X@diverge Z@diverge
  |
  | @  2: 49cb3485fa0c 'C' Y Z
  | |
  | o  1: 6c81ed0049f8 'B' X
  |/
  o  0: 1994f17a630e 'A' Y@diverge
  
  $ hg rebase -s Y -d 3
  rebasing 49cb3485fa0c "C" (Y Z)
  saved backup bundle to $TESTTMP/a1/.hg/strip-backup/49cb3485fa0c-126f3e97-rebase.hg

  $ tglog
  @  3: 17fb3faba63c 'C' Y Z
  |
  o  2: 41acb9dca9eb 'D' W X@diverge
  |
  | o  1: 6c81ed0049f8 'B' X
  |/
  o  0: 1994f17a630e 'A' Y@diverge
  
Do not try to keep active but deleted divergent bookmark

  $ cd ..
  $ hg clone -q a a4

  $ cd a4
  $ hg up -q 2
  $ hg book W@diverge

  $ hg rebase -s W -d .
  rebasing 41acb9dca9eb "D" (W tip)
  saved backup bundle to $TESTTMP/a4/.hg/strip-backup/41acb9dca9eb-b35a6a63-rebase.hg

  $ hg bookmarks
     W                         3:0d3554f74897
     X                         1:6c81ed0049f8
     Y                         2:49cb3485fa0c
     Z                         2:49cb3485fa0c

Keep bookmarks to the correct rebased changeset

  $ cd ..
  $ hg clone -q a a2

  $ cd a2
  $ hg up -q Z

  $ hg rebase -s 1 -d 3
  rebasing 6c81ed0049f8 "B" (X)
  rebasing 49cb3485fa0c "C" (Y Z)
  saved backup bundle to $TESTTMP/a2/.hg/strip-backup/6c81ed0049f8-a687065f-rebase.hg

  $ tglog
  @  3: 3d5fa227f4b5 'C' Y Z
  |
  o  2: e926fccfa8ec 'B' X
  |
  o  1: 41acb9dca9eb 'D' W
  |
  o  0: 1994f17a630e 'A'
  

Keep active bookmark on the correct changeset

  $ cd ..
  $ hg clone -q a a3

  $ cd a3
  $ hg up -q X

  $ hg rebase -d W
  rebasing 6c81ed0049f8 "B" (X)
  rebasing 49cb3485fa0c "C" (Y Z)
  saved backup bundle to $TESTTMP/a3/.hg/strip-backup/6c81ed0049f8-a687065f-rebase.hg

  $ tglog
  o  3: 3d5fa227f4b5 'C' Y Z
  |
  @  2: e926fccfa8ec 'B' X
  |
  o  1: 41acb9dca9eb 'D' W
  |
  o  0: 1994f17a630e 'A'
  
  $ hg bookmarks
     W                         1:41acb9dca9eb
   * X                         2:e926fccfa8ec
     Y                         3:3d5fa227f4b5
     Z                         3:3d5fa227f4b5

rebase --continue with bookmarks present (issue3802)

  $ hg up 2
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark X)
  $ echo 'C' > c
  $ hg add c
  $ hg ci -m 'other C'
  $ hg up 3
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg rebase --dest 4
  rebasing 3d5fa227f4b5 "C" (Y Z)
  merging c
  warning: 1 conflicts while merging c! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ echo 'c' > c
  $ hg resolve --mark c
  (no more unresolved files)
  continue: hg rebase --continue
  $ hg rebase --continue
  rebasing 3d5fa227f4b5 "C" (Y Z)
  saved backup bundle to $TESTTMP/a3/.hg/strip-backup/3d5fa227f4b5-c6ea2371-rebase.hg
  $ tglog
  @  4: 45c0f0ec1203 'C' Y Z
  |
  o  3: b0e10b7175fd 'other C'
  |
  o  2: e926fccfa8ec 'B' X
  |
  o  1: 41acb9dca9eb 'D' W
  |
  o  0: 1994f17a630e 'A'
  

ensure that bookmarks given the names of revset functions can be used
as --rev arguments (issue3950)

  $ hg update -q 3
  $ echo bimble > bimble
  $ hg add bimble
  $ hg commit -q -m 'bisect'
  $ echo e >> bimble
  $ hg ci -m bisect2
  $ echo e >> bimble
  $ hg ci -m bisect3
  $ hg book bisect
  $ hg update -q Y
  $ hg rebase -r '"bisect"^^::"bisect"^' -r bisect -d Z
  rebasing 345c90f326a4 "bisect"
  rebasing f677a2907404 "bisect2"
  rebasing 325c16001345 "bisect3" (bisect tip)
  saved backup bundle to $TESTTMP/a3/.hg/strip-backup/345c90f326a4-b4840586-rebase.hg

Bookmark and working parent get moved even if --keep is set (issue5682)

  $ hg init $TESTTMP/book-keep
  $ cd $TESTTMP/book-keep
  $ drawdag <<'EOS'
  > B C
  > |/
  > A
  > EOS
  $ hg up -q $B
  $ tglog
  o  2: dc0947a82db8 'C'
  |
  | @  1: 112478962961 'B'
  |/
  o  0: 426bada5c675 'A'
  
  $ hg rebase -r $B -d $C --keep
  rebasing 112478962961 "B"
  $ tglog
  @  3: 9769fc65c4c5 'B'
  |
  o  2: dc0947a82db8 'C'
  |
  | o  1: 112478962961 'B'
  |/
  o  0: 426bada5c675 'A'
  

