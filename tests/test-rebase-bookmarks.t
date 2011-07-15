  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > graphlog=
  > rebase=
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

  $ hg tglog 
  @  3: 'D' bookmarks:
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

  $ hg rebase --detach -s Y -d 3
  saved backup bundle to $TESTTMP/a1/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog 
  @  3: 'C' bookmarks: Y Z
  |
  o  2: 'D' bookmarks:
  |
  | o  1: 'B' bookmarks: X
  |/
  o  0: 'A' bookmarks:
  
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
  o  1: 'D' bookmarks:
  |
  o  0: 'A' bookmarks:
  
