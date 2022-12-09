#chg-compatible
#debugruntest-compatible
  $ setconfig experimental.allowfilepeer=True

  $ enable rebase

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

  $ hg up -q 'desc(A)'

  $ echo d > d
  $ hg ci -Am D
  adding d

  $ hg book W

  $ tglog
  @  41acb9dca9eb 'D' W
  │
  │ o  49cb3485fa0c 'C' Y Z
  │ │
  │ o  6c81ed0049f8 'B' X
  ├─╯
  o  1994f17a630e 'A'
  

Move only rebased bookmarks

  $ cd ..
  $ hg clone -q a a1

  $ cd a1
  $ hg up -q Z

Test deleting divergent bookmarks from dest (issue3685)

  $ hg book -r 'desc(D)' Z@diverge

... and also test that bookmarks not on dest or not being moved aren't deleted

  $ hg book -r 'desc(D)' X@diverge
  $ hg book -r 'desc(A)' Y@diverge

  $ tglog
  o  41acb9dca9eb 'D' W X@diverge Z@diverge
  │
  │ @  49cb3485fa0c 'C' Y Z
  │ │
  │ o  6c81ed0049f8 'B' X
  ├─╯
  o  1994f17a630e 'A' Y@diverge
  
  $ hg rebase -s Y -d 'desc(D)'
  rebasing 49cb3485fa0c "C" (Y Z)

  $ tglog
  @  17fb3faba63c 'C' Y Z
  │
  o  41acb9dca9eb 'D' W X@diverge
  │
  │ o  6c81ed0049f8 'B' X
  ├─╯
  o  1994f17a630e 'A' Y@diverge
  
Do not try to keep active but deleted divergent bookmark

  $ cd ..
  $ hg clone -q a a4

  $ cd a4
  $ hg up -q 'desc(C)'
  $ hg book W@diverge

  $ hg rebase -s W -d .
  rebasing 41acb9dca9eb "D" (W)

  $ hg bookmarks
     W                         0d3554f74897
     X                         6c81ed0049f8
     Y                         49cb3485fa0c
     Z                         49cb3485fa0c

Keep bookmarks to the correct rebased changeset

  $ cd ..
  $ hg clone -q a a2

  $ cd a2
  $ hg up -q Z

  $ hg rebase -s 'desc(B)' -d 'desc(D)'
  rebasing 6c81ed0049f8 "B" (X)
  rebasing 49cb3485fa0c "C" (Y Z)

  $ tglog
  @  3d5fa227f4b5 'C' Y Z
  │
  o  e926fccfa8ec 'B' X
  │
  o  41acb9dca9eb 'D' W
  │
  o  1994f17a630e 'A'
  

Keep active bookmark on the correct changeset

  $ cd ..
  $ hg clone -q a a3

  $ cd a3
  $ hg up -q X

  $ hg rebase -d W
  rebasing 6c81ed0049f8 "B" (X)
  rebasing 49cb3485fa0c "C" (Y Z)

  $ tglog
  o  3d5fa227f4b5 'C' Y Z
  │
  @  e926fccfa8ec 'B' X
  │
  o  41acb9dca9eb 'D' W
  │
  o  1994f17a630e 'A'
  
  $ hg bookmarks
     W                         41acb9dca9eb
   * X                         e926fccfa8ec
     Y                         3d5fa227f4b5
     Z                         3d5fa227f4b5

rebase --continue with bookmarks present (issue3802)

  $ hg up 'bookmark(X)'
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark X)
  $ echo 'C' > c
  $ hg add c
  $ hg ci -m 'other C'
  $ hg up 'bookmark(Y)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg rebase --dest 'desc(other)'
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
  $ tglog
  @  45c0f0ec1203 'C' Y Z
  │
  o  b0e10b7175fd 'other C'
  │
  o  e926fccfa8ec 'B' X
  │
  o  41acb9dca9eb 'D' W
  │
  o  1994f17a630e 'A'
  

ensure that bookmarks given the names of revset functions can be used
as --rev arguments (issue3950)

  $ hg goto -q 'desc(other)'
  $ echo bimble > bimble
  $ hg add bimble
  $ hg commit -q -m 'bisect'
  $ echo e >> bimble
  $ hg ci -m bisect2
  $ echo e >> bimble
  $ hg ci -m bisect3
  $ hg book bisect
  $ hg goto -q Y
  $ hg rebase -r '"bisect"^^::"bisect"^' -r bisect -d Z
  rebasing 345c90f326a4 "bisect"
  rebasing f677a2907404 "bisect2"
  rebasing 325c16001345 "bisect3" (bisect)

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
  o  dc0947a82db8 'C'
  │
  │ @  112478962961 'B'
  ├─╯
  o  426bada5c675 'A'
  
  $ hg rebase -r $B -d $C --keep
  rebasing 112478962961 "B"
  $ tglog
  @  9769fc65c4c5 'B'
  │
  o  dc0947a82db8 'C'
  │
  │ o  112478962961 'B'
  ├─╯
  o  426bada5c675 'A'
  

