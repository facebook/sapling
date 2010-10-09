  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > graphlog=
  > rebase=
  > 
  > [alias]
  > tglog  = log -G --template "{rev}: '{desc}' {branches}\n"
  > theads = heads --template "{rev}: '{desc}' {branches}\n"
  > EOF

  $ hg init a
  $ cd a

  $ echo a > a
  $ hg ci -Am A
  adding a

  $ hg branch branch1
  marked working directory as branch branch1
  $ hg ci -m 'branch1'

  $ echo b > b
  $ hg ci -Am B
  adding b

  $ hg up -q 0

  $ hg branch branch2
  marked working directory as branch branch2
  $ hg ci -m 'branch2'

  $ echo c > C
  $ hg ci -Am C
  adding C

  $ hg up -q 2

  $ hg branch -f branch2
  marked working directory as branch branch2
  $ echo d > d
  $ hg ci -Am D
  adding d
  created new head

  $ echo e > e
  $ hg ci -Am E
  adding e

  $ hg update default
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved

  $ hg branch branch3
  marked working directory as branch branch3
  $ hg ci -m 'branch3'

  $ echo f > f
  $ hg ci -Am F
  adding f

  $ cd ..


Rebase part of branch2 (5-6) onto branch3 (8):

  $ hg clone -q -u . a a1
  $ cd a1

  $ hg tglog  
  @  8: 'F' branch3
  |
  o  7: 'branch3' branch3
  |
  | o  6: 'E' branch2
  | |
  | o  5: 'D' branch2
  | |
  | | o  4: 'C' branch2
  | | |
  +---o  3: 'branch2' branch2
  | |
  | o  2: 'B' branch1
  | |
  | o  1: 'branch1' branch1
  |/
  o  0: 'A'
  
  $ hg branches
  branch3                        8:05b64c4ca2d8
  branch2                        6:b410fbec727a
  branch1                        2:9d931918fcf7 (inactive)
  default                        0:1994f17a630e (inactive)

  $ hg theads
  8: 'F' branch3
  6: 'E' branch2
  4: 'C' branch2
  2: 'B' branch1
  0: 'A' 

  $ hg rebase --detach -s 5 -d 8
  saved backup bundle to $TESTTMP/a1/.hg/strip-backup/*-backup.hg (glob)

  $ hg branches
  branch3                        8:c1d4b9719987
  branch2                        4:1be2b203ae5e
  branch1                        2:9d931918fcf7
  default                        0:1994f17a630e (inactive)

  $ hg theads
  8: 'E' branch3
  4: 'C' branch2
  2: 'B' branch1
  0: 'A' 

  $ hg tglog  
  @  8: 'E' branch3
  |
  o  7: 'D' branch3
  |
  o  6: 'F' branch3
  |
  o  5: 'branch3' branch3
  |
  | o  4: 'C' branch2
  | |
  | o  3: 'branch2' branch2
  |/
  | o  2: 'B' branch1
  | |
  | o  1: 'branch1' branch1
  |/
  o  0: 'A'
  
  $ cd ..


Rebase head of branch3 (8) onto branch2 (6):

  $ hg clone -q -u . a a2
  $ cd a2

  $ hg tglog
  @  8: 'F' branch3
  |
  o  7: 'branch3' branch3
  |
  | o  6: 'E' branch2
  | |
  | o  5: 'D' branch2
  | |
  | | o  4: 'C' branch2
  | | |
  +---o  3: 'branch2' branch2
  | |
  | o  2: 'B' branch1
  | |
  | o  1: 'branch1' branch1
  |/
  o  0: 'A'
  
  $ hg rebase --detach -s 8 -d 6
  saved backup bundle to $TESTTMP/a2/.hg/strip-backup/*-backup.hg (glob)

  $ hg branches
  branch2                        8:e1e80ed73210
  branch3                        7:75fd7b643dce
  branch1                        2:9d931918fcf7 (inactive)
  default                        0:1994f17a630e (inactive)

  $ hg theads
  8: 'F' branch2
  7: 'branch3' branch3
  4: 'C' branch2
  2: 'B' branch1
  0: 'A' 

  $ hg tglog
  @  8: 'F' branch2
  |
  | o  7: 'branch3' branch3
  | |
  o |  6: 'E' branch2
  | |
  o |  5: 'D' branch2
  | |
  | | o  4: 'C' branch2
  | | |
  | | o  3: 'branch2' branch2
  | |/
  o |  2: 'B' branch1
  | |
  o |  1: 'branch1' branch1
  |/
  o  0: 'A'
  
  $ hg verify -q

  $ cd ..


Rebase entire branch3 (7-8) onto branch2 (6):

  $ hg clone -q -u . a a3
  $ cd a3

  $ hg tglog
  @  8: 'F' branch3
  |
  o  7: 'branch3' branch3
  |
  | o  6: 'E' branch2
  | |
  | o  5: 'D' branch2
  | |
  | | o  4: 'C' branch2
  | | |
  +---o  3: 'branch2' branch2
  | |
  | o  2: 'B' branch1
  | |
  | o  1: 'branch1' branch1
  |/
  o  0: 'A'
  
  $ hg rebase --detach -s 7 -d 6
  saved backup bundle to $TESTTMP/a3/.hg/strip-backup/*-backup.hg (glob)

  $ hg branches
  branch2                        7:e1e80ed73210
  branch1                        2:9d931918fcf7 (inactive)
  default                        0:1994f17a630e (inactive)

  $ hg theads
  7: 'F' branch2
  4: 'C' branch2
  2: 'B' branch1
  0: 'A' 

  $ hg tglog   
  @  7: 'F' branch2
  |
  o  6: 'E' branch2
  |
  o  5: 'D' branch2
  |
  | o  4: 'C' branch2
  | |
  | o  3: 'branch2' branch2
  | |
  o |  2: 'B' branch1
  | |
  o |  1: 'branch1' branch1
  |/
  o  0: 'A'
  
  $ hg verify -q

