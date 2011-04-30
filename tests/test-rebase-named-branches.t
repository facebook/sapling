  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > graphlog=
  > rebase=
  > 
  > [alias]
  > tglog = log -G --template "{rev}: '{desc}' {branches}\n"
  > EOF

  $ hg init a
  $ cd a
  $ hg unbundle $TESTDIR/bundles/rebase.hg
  adding changesets
  adding manifests
  adding file changes
  added 6 changesets with 5 changes to 5 files (+2 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hg up tip
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ..


Rebasing descendant onto ancestor across different named branches

  $ hg clone -q -u . a a1

  $ cd a1

  $ hg branch dev
  marked working directory as branch dev

  $ echo x > x

  $ hg add x

  $ hg ci -m 'extra named branch'

  $ hg tglog
  @  6: 'extra named branch' dev
  |
  o  5: 'F'
  |
  | o  4: 'E'
  |/|
  o |  3: 'D'
  | |
  | o  2: 'C'
  |/
  | o  1: 'B'
  |/
  o  0: 'A'
  
  $ hg rebase -s 6 -d 5
  saved backup bundle to $TESTTMP/a1/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  6: 'extra named branch'
  |
  o  5: 'F'
  |
  | o  4: 'E'
  |/|
  o |  3: 'D'
  | |
  | o  2: 'C'
  |/
  | o  1: 'B'
  |/
  o  0: 'A'
  
  $ cd ..
 
Rebasing descendant onto ancestor across the same named branches

  $ hg clone -q -u . a a2

  $ cd a2

  $ echo x > x

  $ hg add x

  $ hg ci -m 'G'

  $ hg tglog
  @  6: 'G'
  |
  o  5: 'F'
  |
  | o  4: 'E'
  |/|
  o |  3: 'D'
  | |
  | o  2: 'C'
  |/
  | o  1: 'B'
  |/
  o  0: 'A'
  
  $ hg rebase -s 6 -d 5
  abort: source is descendant of destination
  [255]

  $ cd ..
 
Rebasing ancestor onto descendant across different named branches

  $ hg clone -q -u . a a3

  $ cd a3

  $ hg branch dev
  marked working directory as branch dev

  $ echo x > x

  $ hg add x

  $ hg ci -m 'extra named branch'

  $ hg tglog
  @  6: 'extra named branch' dev
  |
  o  5: 'F'
  |
  | o  4: 'E'
  |/|
  o |  3: 'D'
  | |
  | o  2: 'C'
  |/
  | o  1: 'B'
  |/
  o  0: 'A'
  
  $ hg rebase -s 5 -d 6
  abort: source is ancestor of destination
  [255]

  $ cd ..
 

