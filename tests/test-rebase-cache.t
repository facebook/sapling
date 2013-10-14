  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > graphlog=
  > rebase=
  > mq=
  > 
  > [phases]
  > publish=False
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
  (branches are permanent and global, did you want a bookmark?)
  $ hg ci -m 'branch1'

  $ echo b > b
  $ hg ci -Am B
  adding b

  $ hg up -q 0

  $ hg branch branch2
  marked working directory as branch branch2
  (branches are permanent and global, did you want a bookmark?)
  $ hg ci -m 'branch2'

  $ echo c > C
  $ hg ci -Am C
  adding C

  $ hg up -q 2

  $ hg branch -f branch2
  marked working directory as branch branch2
  (branches are permanent and global, did you want a bookmark?)
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
  (branches are permanent and global, did you want a bookmark?)
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
  branch3                        8:4666b71e8e32
  branch2                        6:5097051d331d
  branch1                        2:0a03079c47fd (inactive)
  default                        0:1994f17a630e (inactive)

  $ hg theads
  8: 'F' branch3
  6: 'E' branch2
  4: 'C' branch2
  2: 'B' branch1
  0: 'A' 

  $ hg rebase -s 5 -d 8
  saved backup bundle to $TESTTMP/a1/.hg/strip-backup/*-backup.hg (glob)

  $ hg branches
  branch3                        8:466cdfb14b62
  branch2                        4:e4fdb121d036
  branch1                        2:0a03079c47fd
  default                        0:1994f17a630e (inactive)

  $ hg theads
  8: 'E' branch3
  4: 'C' branch2
  2: 'B' branch1
  0: 'A' 

  $ hg tglog
  o  8: 'E' branch3
  |
  o  7: 'D' branch3
  |
  @  6: 'F' branch3
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
  
  $ hg rebase -s 8 -d 6
  saved backup bundle to $TESTTMP/a2/.hg/strip-backup/*-backup.hg (glob)

  $ hg branches
  branch2                        8:6b4bdc1b5ac0
  branch3                        7:653b9feb4616
  branch1                        2:0a03079c47fd (inactive)
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
  
  $ hg rebase -s 7 -d 6
  saved backup bundle to $TESTTMP/a3/.hg/strip-backup/*-backup.hg (glob)

  $ hg branches
  branch2                        7:6b4bdc1b5ac0
  branch1                        2:0a03079c47fd (inactive)
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

Stripping multiple branches in one go bypasses the fast-case code to
update the branch cache.

  $ hg strip 2
  0 files updated, 0 files merged, 4 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/a3/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  o  3: 'C' branch2
  |
  o  2: 'branch2' branch2
  |
  | @  1: 'branch1' branch1
  |/
  o  0: 'A'
  

  $ hg branches
  branch2                        3:e4fdb121d036
  branch1                        1:63379ac49655
  default                        0:1994f17a630e (inactive)

  $ hg theads
  3: 'C' branch2
  1: 'branch1' branch1
  0: 'A' 

Fast path branchcache code should not be invoked if branches stripped is not
the same as branches remaining.

  $ hg init b
  $ cd b

  $ hg branch branch1
  marked working directory as branch branch1
  (branches are permanent and global, did you want a bookmark?)
  $ hg ci -m 'branch1'

  $ hg branch branch2
  marked working directory as branch branch2
  (branches are permanent and global, did you want a bookmark?)
  $ hg ci -m 'branch2'

  $ hg branch -f branch1
  marked working directory as branch branch1
  (branches are permanent and global, did you want a bookmark?)

  $ echo a > A
  $ hg ci -Am A
  adding A
  created new head

  $ hg tglog
  @  2: 'A' branch1
  |
  o  1: 'branch2' branch2
  |
  o  0: 'branch1' branch1
  

  $ hg theads
  2: 'A' branch1
  1: 'branch2' branch2

  $ hg strip 2
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/a3/b/.hg/strip-backup/*-backup.hg (glob)

  $ hg theads
  1: 'branch2' branch2
  0: 'branch1' branch1


Make sure requesting to strip a revision already stripped does not confuse things.
Try both orders.

  $ cd ..

  $ hg init c
  $ cd c

  $ echo a > a
  $ hg ci -Am A
  adding a
  $ echo b > b
  $ hg ci -Am B
  adding b
  $ echo c > c
  $ hg ci -Am C
  adding c
  $ echo d > d
  $ hg ci -Am D
  adding d
  $ echo e > e
  $ hg ci -Am E
  adding e

  $ hg tglog
  @  4: 'E'
  |
  o  3: 'D'
  |
  o  2: 'C'
  |
  o  1: 'B'
  |
  o  0: 'A'
  

  $ hg strip 3 4
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/a3/c/.hg/strip-backup/*-backup.hg (glob)

  $ hg theads
  2: 'C' 

  $ hg strip 2 1
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/a3/c/.hg/strip-backup/*-backup.hg (glob)

  $ hg theads
  0: 'A' 

Make sure rebase does not break for phase/filter related reason
----------------------------------------------------------------
(issue3858)

  $ cd ..

  $ cat >> $HGRCPATH << EOF
  > [ui]
  > logtemplate={rev} {desc} {phase}\n
  > EOF


  $ hg init c4
  $ cd c4

  $ echo a > a
  $ hg ci -Am A
  adding a
  $ echo b > b
  $ hg ci -Am B
  adding b
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo c > c
  $ hg ci -Am C
  adding c
  created new head
  $ hg up 1
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg merge
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m d
  $ hg up 2
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo e > e
  $ hg ci -Am E
  adding e
  created new head
  $ hg merge 3
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m F
  $ hg up 3
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo g > g
  $ hg ci -Am G
  adding g
  created new head
  $ echo h > h
  $ hg ci -Am H
  adding h
  $ hg up 5
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ echo i > i
  $ hg ci -Am I
  adding i

Turn most changeset public

  $ hg ph -p 7

  $ hg heads
  8 I draft
  7 H public
  $ hg log -G
  @  8 I draft
  |
  | o  7 H public
  | |
  | o  6 G public
  | |
  o |  5 F draft
  |\|
  o |  4 E draft
  | |
  | o  3 d public
  |/|
  o |  2 C public
  | |
  | o  1 B public
  |/
  o  0 A public
  

  $ hg rebase --dest 7 --source 5
  saved backup bundle to $TESTTMP/a3/c4/.hg/strip-backup/*-backup.hg (glob)
