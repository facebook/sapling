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


Rebasing
B onto F - simple rebase:

  $ hg clone -q -u . a a1
  $ cd a1

  $ hg tglog
  @  5: 'F'
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
  
  $ hg rebase -s 1 -d 5
  saved backup bundle to $TESTTMP/a1/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  5: 'B'
  |
  o  4: 'F'
  |
  | o  3: 'E'
  |/|
  o |  2: 'D'
  | |
  | o  1: 'C'
  |/
  o  0: 'A'
  
  $ cd ..


B onto D - intermediate point:

  $ hg clone -q -u . a a2
  $ cd a2

  $ hg rebase -s 1 -d 3
  saved backup bundle to $TESTTMP/a2/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  5: 'B'
  |
  | o  4: 'F'
  |/
  | o  3: 'E'
  |/|
  o |  2: 'D'
  | |
  | o  1: 'C'
  |/
  o  0: 'A'
  
  $ cd ..


C onto F - skip of E:

  $ hg clone -q -u . a a3
  $ cd a3

  $ hg rebase -s 2 -d 5
  saved backup bundle to $TESTTMP/a3/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  4: 'C'
  |
  o  3: 'F'
  |
  o  2: 'D'
  |
  | o  1: 'B'
  |/
  o  0: 'A'
  
  $ cd ..


D onto C - rebase of a branching point (skip E):

  $ hg clone -q -u . a a4
  $ cd a4

  $ hg rebase -s 3 -d 2
  saved backup bundle to $TESTTMP/a4/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  4: 'F'
  |
  o  3: 'D'
  |
  o  2: 'C'
  |
  | o  1: 'B'
  |/
  o  0: 'A'
  
  $ cd ..


E onto F - merged revision having a parent in ancestors of target:

  $ hg clone -q -u . a a5
  $ cd a5

  $ hg rebase -s 4 -d 5
  saved backup bundle to $TESTTMP/a5/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @    5: 'E'
  |\
  | o  4: 'F'
  | |
  | o  3: 'D'
  | |
  o |  2: 'C'
  |/
  | o  1: 'B'
  |/
  o  0: 'A'
  
  $ cd ..


D onto B - E maintains C as parent:

  $ hg clone -q -u . a a6
  $ cd a6

  $ hg rebase -s 3 -d 1
  saved backup bundle to $TESTTMP/a6/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  5: 'F'
  |
  | o  4: 'E'
  |/|
  o |  3: 'D'
  | |
  | o  2: 'C'
  | |
  o |  1: 'B'
  |/
  o  0: 'A'
  
  $ cd ..


These will fail (using --source):

E onto D - rebase onto an ancestor:

  $ hg clone -q -u . a a7
  $ cd a7

  $ hg rebase -s 4 -d 3
  abort: source is descendant of destination
  [255]

D onto E - rebase onto a descendant:

  $ hg rebase -s 3 -d 4
  abort: source is ancestor of destination
  [255]

E onto B - merge revision with both parents not in ancestors of target:

  $ hg rebase -s 4 -d 1
  abort: cannot use revision 4 as base, result would have 3 parents
  [255]


These will abort gracefully (using --base):

E onto E - rebase onto same changeset:

  $ hg rebase -b 4 -d 4
  nothing to rebase
  [1]

E onto D - rebase onto an ancestor:

  $ hg rebase -b 4 -d 3
  nothing to rebase
  [1]

D onto E - rebase onto a descendant:

  $ hg rebase -b 3 -d 4
  nothing to rebase
  [1]

