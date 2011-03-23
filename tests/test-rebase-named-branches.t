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

  $ echo A > A
  $ hg ci -Am A
  adding A

  $ echo B > B
  $ hg ci -Am B
  adding B

  $ hg up -q -C 0

  $ echo C > C
  $ hg ci -Am C
  adding C
  created new head

  $ hg up -q -C 0

  $ echo D > D
  $ hg ci -Am D
  adding D
  created new head

  $ hg merge -r 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ hg ci -m E

  $ hg up -q -C 3

  $ echo F > F
  $ hg ci -Am F
  adding F
  created new head

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
 

