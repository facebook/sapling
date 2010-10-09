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

  $ echo c1 > c1
  $ hg ci -Am c1
  adding c1

  $ echo c2 > c2
  $ hg ci -Am c2
  adding c2

  $ echo c3 > c3
  $ hg ci -Am c3
  adding c3

  $ hg up -q -C 1

  $ echo l1 > l1
  $ hg ci -Am l1
  adding l1
  created new head

  $ echo l2 > l2
  $ hg ci -Am l2
  adding l2

  $ echo l3 > l3
  $ hg ci -Am l3
  adding l3

  $ hg up -q -C 2

  $ echo r1 > r1
  $ hg ci -Am r1
  adding r1

  $ echo r2 > r2
  $ hg ci -Am r2
  adding r2

  $ hg tglog
  @  7: 'r2'
  |
  o  6: 'r1'
  |
  | o  5: 'l3'
  | |
  | o  4: 'l2'
  | |
  | o  3: 'l1'
  | |
  o |  2: 'c3'
  |/
  o  1: 'c2'
  |
  o  0: 'c1'
  
  $ cd ..


These fail:

  $ hg clone -q -u . a a1
  $ cd a1

  $ hg rebase --continue --abort
  abort: cannot use both abort and continue
  [255]

  $ hg rebase --continue --collapse
  abort: cannot use collapse with continue or abort
  [255]

  $ hg rebase --continue --dest 4
  abort: abort and continue do not allow specifying revisions
  [255]

  $ hg rebase --base 5 --source 4
  abort: cannot specify both a revision and a base
  [255]

  $ hg rebase
  nothing to rebase
  [1]

  $ hg up -q 6

  $ hg rebase
  nothing to rebase
  [1]


These work:

Rebase with no arguments (from 3 onto 7):

  $ hg up -q -C 5

  $ hg rebase
  saved backup bundle to $TESTTMP/a1/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  7: 'l3'
  |
  o  6: 'l2'
  |
  o  5: 'l1'
  |
  o  4: 'r2'
  |
  o  3: 'r1'
  |
  o  2: 'c3'
  |
  o  1: 'c2'
  |
  o  0: 'c1'
  
Try to rollback after a rebase (fail):

  $ hg rollback
  no rollback information available
  [1]

  $ cd ..


Rebase with base == '.' => same as no arguments (from 3 onto 7):

  $ hg clone -q -u 5 a a2
  $ cd a2

  $ hg rebase --base .
  saved backup bundle to $TESTTMP/a2/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  7: 'l3'
  |
  o  6: 'l2'
  |
  o  5: 'l1'
  |
  o  4: 'r2'
  |
  o  3: 'r1'
  |
  o  2: 'c3'
  |
  o  1: 'c2'
  |
  o  0: 'c1'
  
  $ cd ..


Rebase with dest == `hg branch` => same as no arguments (from 3 onto 7):

  $ hg clone -q -u 5 a a3
  $ cd a3

  $ hg rebase --dest `hg branch`
  saved backup bundle to $TESTTMP/a3/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  7: 'l3'
  |
  o  6: 'l2'
  |
  o  5: 'l1'
  |
  o  4: 'r2'
  |
  o  3: 'r1'
  |
  o  2: 'c3'
  |
  o  1: 'c2'
  |
  o  0: 'c1'
  
  $ cd ..


Specify only source (from 4 onto 7):

  $ hg clone -q -u . a a4
  $ cd a4

  $ hg rebase --source 4
  saved backup bundle to $TESTTMP/a4/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  7: 'l3'
  |
  o    6: 'l2'
  |\
  | o  5: 'r2'
  | |
  | o  4: 'r1'
  | |
  o |  3: 'l1'
  | |
  | o  2: 'c3'
  |/
  o  1: 'c2'
  |
  o  0: 'c1'
  
  $ cd ..


Specify only dest (from 3 onto 6):

  $ hg clone -q -u 5 a a5
  $ cd a5

  $ hg rebase --dest 6
  saved backup bundle to $TESTTMP/a5/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  7: 'l3'
  |
  o  6: 'l2'
  |
  o  5: 'l1'
  |
  | o  4: 'r2'
  |/
  o  3: 'r1'
  |
  o  2: 'c3'
  |
  o  1: 'c2'
  |
  o  0: 'c1'
  
  $ cd ..


Specify only base (from 3 onto 7):

  $ hg clone -q -u . a a6
  $ cd a6

  $ hg rebase --base 5
  saved backup bundle to $TESTTMP/a6/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  7: 'l3'
  |
  o  6: 'l2'
  |
  o  5: 'l1'
  |
  o  4: 'r2'
  |
  o  3: 'r1'
  |
  o  2: 'c3'
  |
  o  1: 'c2'
  |
  o  0: 'c1'
  
  $ cd ..


Specify source and dest (from 4 onto 6):

  $ hg clone -q -u . a a7
  $ cd a7

  $ hg rebase --source 4 --dest 6
  saved backup bundle to $TESTTMP/a7/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  7: 'l3'
  |
  o    6: 'l2'
  |\
  | | o  5: 'r2'
  | |/
  | o  4: 'r1'
  | |
  o |  3: 'l1'
  | |
  | o  2: 'c3'
  |/
  o  1: 'c2'
  |
  o  0: 'c1'
  
  $ cd ..


Specify base and dest (from 3 onto 6):

  $ hg clone -q -u . a a8
  $ cd a8

  $ hg rebase --base 4 --dest 6
  saved backup bundle to $TESTTMP/a8/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  7: 'l3'
  |
  o  6: 'l2'
  |
  o  5: 'l1'
  |
  | o  4: 'r2'
  |/
  o  3: 'r1'
  |
  o  2: 'c3'
  |
  o  1: 'c2'
  |
  o  0: 'c1'
  
  $ cd ..

