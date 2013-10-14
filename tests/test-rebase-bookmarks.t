  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > graphlog=
  > rebase=
  > 
  > [phases]
  > publish=False
  > 
  > [alias]
  > tglog = log -G --template "{rev}: '{desc}' bookmarks: {bookmarks}\n"
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
  created new head

  $ hg book W

  $ hg tglog
  @  3: 'D' bookmarks: W
  |
  | o  2: 'C' bookmarks: Y Z
  | |
  | o  1: 'B' bookmarks: X
  |/
  o  0: 'A' bookmarks:
  

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

  $ hg tglog
  o  3: 'D' bookmarks: W X@diverge Z@diverge
  |
  | @  2: 'C' bookmarks: Y Z
  | |
  | o  1: 'B' bookmarks: X
  |/
  o  0: 'A' bookmarks: Y@diverge
  
  $ hg rebase -s Y -d 3
  saved backup bundle to $TESTTMP/a1/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  3: 'C' bookmarks: Y Z
  |
  o  2: 'D' bookmarks: W X@diverge
  |
  | o  1: 'B' bookmarks: X
  |/
  o  0: 'A' bookmarks: Y@diverge
  
Keep bookmarks to the correct rebased changeset

  $ cd ..
  $ hg clone -q a a2

  $ cd a2
  $ hg up -q Z

  $ hg rebase -s 1 -d 3
  saved backup bundle to $TESTTMP/a2/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  3: 'C' bookmarks: Y Z
  |
  o  2: 'B' bookmarks: X
  |
  o  1: 'D' bookmarks: W
  |
  o  0: 'A' bookmarks:
  

Keep active bookmark on the correct changeset

  $ cd ..
  $ hg clone -q a a3

  $ cd a3
  $ hg up -q X

  $ hg rebase -d W
  saved backup bundle to $TESTTMP/a3/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  o  3: 'C' bookmarks: Y Z
  |
  @  2: 'B' bookmarks: X
  |
  o  1: 'D' bookmarks: W
  |
  o  0: 'A' bookmarks:
  
  $ hg bookmarks
     W                         1:41acb9dca9eb
   * X                         2:e926fccfa8ec
     Y                         3:3d5fa227f4b5
     Z                         3:3d5fa227f4b5

rebase --continue with bookmarks present (issue3802)

  $ hg up 2
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo 'C' > c
  $ hg add c
  $ hg ci -m 'other C'
  created new head
  $ hg up 3
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg rebase
  merging c
  warning: conflicts during merge.
  merging c incomplete! (edit conflicts, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ echo 'c' > c
  $ hg resolve --mark c
  $ hg rebase --continue
  saved backup bundle to $TESTTMP/a3/.hg/strip-backup/3d5fa227f4b5-backup.hg (glob)
  $ hg tglog
  @  4: 'C' bookmarks: Y Z
  |
  o  3: 'other C' bookmarks:
  |
  o  2: 'B' bookmarks: X
  |
  o  1: 'D' bookmarks: W
  |
  o  0: 'A' bookmarks:
  

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
  saved backup bundle to $TESTTMP/a3/.hg/strip-backup/345c90f326a4-backup.hg (glob)
