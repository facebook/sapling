  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > graphlog=
  > rebase=
  > 
  > [alias]
  > tglog = log -G --template "{rev}: '{desc}' {branches}\n"
  > EOF

Create repo a:

  $ hg init a
  $ cd a
  $ hg unbundle $TESTDIR/bundles/rebase.hg
  adding changesets
  adding manifests
  adding file changes
  added 8 changesets with 7 changes to 7 files (+2 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hg up tip
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg tglog
  @  7: 'H'
  |
  | o  6: 'G'
  |/|
  o |  5: 'F'
  | |
  | o  4: 'E'
  |/
  | o  3: 'D'
  | |
  | o  2: 'C'
  | |
  | o  1: 'B'
  |/
  o  0: 'A'
  
  $ cd ..


Rebasing B onto H:

  $ hg clone -q -u 3 a a1
  $ cd a1

  $ hg rebase --collapse --keepbranches
  saved backup bundle to $TESTTMP/a1/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  5: 'Collapsed revision
  |  * B
  |  * C
  |  * D'
  o  4: 'H'
  |
  | o  3: 'G'
  |/|
  o |  2: 'F'
  | |
  | o  1: 'E'
  |/
  o  0: 'A'
  
  $ hg manifest
  A
  B
  C
  D
  F
  H

  $ cd ..


Rebasing E onto H:

  $ hg clone -q -u . a a2
  $ cd a2

  $ hg rebase --source 4 --collapse
  saved backup bundle to $TESTTMP/a2/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  6: 'Collapsed revision
  |  * E
  |  * G'
  o  5: 'H'
  |
  o  4: 'F'
  |
  | o  3: 'D'
  | |
  | o  2: 'C'
  | |
  | o  1: 'B'
  |/
  o  0: 'A'
  
  $ hg manifest
  A
  E
  F
  H

  $ cd ..

Rebasing G onto H with custom message:

  $ hg clone -q -u . a a3
  $ cd a3

  $ hg rebase --base 6 -m 'custom message'
  abort: message can only be specified with collapse
  [255]

  $ hg rebase --source 4 --collapse -m 'custom message'
  saved backup bundle to $TESTTMP/a3/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  6: 'custom message'
  |
  o  5: 'H'
  |
  o  4: 'F'
  |
  | o  3: 'D'
  | |
  | o  2: 'C'
  | |
  | o  1: 'B'
  |/
  o  0: 'A'
  
  $ hg manifest
  A
  E
  F
  H

  $ cd ..

Create repo b:

  $ hg init b
  $ cd b

  $ echo A > A
  $ hg ci -Am A
  adding A
  $ echo B > B
  $ hg ci -Am B
  adding B

  $ hg up -q 0

  $ echo C > C
  $ hg ci -Am C
  adding C
  created new head

  $ hg merge
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ echo D > D
  $ hg ci -Am D
  adding D

  $ hg up -q 1

  $ echo E > E
  $ hg ci -Am E
  adding E
  created new head

  $ echo F > F
  $ hg ci -Am F
  adding F

  $ hg merge
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m G

  $ hg up -q 0

  $ echo H > H
  $ hg ci -Am H
  adding H
  created new head

  $ hg tglog
  @  7: 'H'
  |
  | o    6: 'G'
  | |\
  | | o  5: 'F'
  | | |
  | | o  4: 'E'
  | | |
  | o |  3: 'D'
  | |\|
  | o |  2: 'C'
  |/ /
  | o  1: 'B'
  |/
  o  0: 'A'
  
  $ cd ..


Rebase and collapse - more than one external (fail):

  $ hg clone -q -u . b b1
  $ cd b1

  $ hg rebase -s 2 --collapse
  abort: unable to collapse, there is more than one external parent
  [255]

Rebase and collapse - E onto H:

  $ hg rebase -s 4 --collapse
  saved backup bundle to $TESTTMP/b1/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @    5: 'Collapsed revision
  |\   * E
  | |  * F
  | |  * G'
  | o  4: 'H'
  | |
  o |    3: 'D'
  |\ \
  | o |  2: 'C'
  | |/
  o /  1: 'B'
  |/
  o  0: 'A'
  
  $ hg manifest
  A
  B
  C
  D
  E
  F
  H

  $ cd ..


Create repo c:

  $ hg init c
  $ cd c

  $ echo A > A
  $ hg ci -Am A
  adding A
  $ echo B > B
  $ hg ci -Am B
  adding B

  $ hg up -q 0

  $ echo C > C
  $ hg ci -Am C
  adding C
  created new head

  $ hg merge
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ echo D > D
  $ hg ci -Am D
  adding D

  $ hg up -q 1

  $ echo E > E
  $ hg ci -Am E
  adding E
  created new head
  $ echo F > E
  $ hg ci -m 'F'

  $ echo G > G
  $ hg ci -Am G
  adding G

  $ hg merge
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ hg ci -m H

  $ hg up -q 0

  $ echo I > I
  $ hg ci -Am I
  adding I
  created new head

  $ hg tglog
  @  8: 'I'
  |
  | o    7: 'H'
  | |\
  | | o  6: 'G'
  | | |
  | | o  5: 'F'
  | | |
  | | o  4: 'E'
  | | |
  | o |  3: 'D'
  | |\|
  | o |  2: 'C'
  |/ /
  | o  1: 'B'
  |/
  o  0: 'A'
  
  $ cd ..


Rebase and collapse - E onto I:

  $ hg clone -q -u . c c1
  $ cd c1

  $ hg rebase -s 4 --collapse
  merging E
  saved backup bundle to $TESTTMP/c1/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @    5: 'Collapsed revision
  |\   * E
  | |  * F
  | |  * G
  | |  * H'
  | o  4: 'I'
  | |
  o |    3: 'D'
  |\ \
  | o |  2: 'C'
  | |/
  o /  1: 'B'
  |/
  o  0: 'A'
  
  $ hg manifest
  A
  B
  C
  D
  E
  G
  I

  $ cat E
  F

  $ cd ..


Create repo d:

  $ hg init d
  $ cd d

  $ echo A > A
  $ hg ci -Am A
  adding A
  $ echo B > B
  $ hg ci -Am B
  adding B
  $ echo C > C
  $ hg ci -Am C
  adding C

  $ hg up -q 1

  $ echo D > D
  $ hg ci -Am D
  adding D
  created new head
  $ hg merge
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ hg ci -m E

  $ hg up -q 0

  $ echo F > F
  $ hg ci -Am F
  adding F
  created new head

  $ hg tglog
  @  5: 'F'
  |
  | o    4: 'E'
  | |\
  | | o  3: 'D'
  | | |
  | o |  2: 'C'
  | |/
  | o  1: 'B'
  |/
  o  0: 'A'
  
  $ cd ..


Rebase and collapse - B onto F:

  $ hg clone -q -u . d d1
  $ cd d1

  $ hg rebase -s 1 --collapse
  saved backup bundle to $TESTTMP/d1/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  2: 'Collapsed revision
  |  * B
  |  * C
  |  * D
  |  * E'
  o  1: 'F'
  |
  o  0: 'A'
  
  $ hg manifest
  A
  B
  C
  D
  F

Interactions between collapse and keepbranches
  $ cd ..
  $ hg init e
  $ cd e
  $ echo 'a' > a
  $ hg ci -Am 'A'
  adding a

  $ hg branch '1'
  marked working directory as branch 1
  $ echo 'b' > b
  $ hg ci -Am 'B'
  adding b

  $ hg branch '2'
  marked working directory as branch 2
  $ echo 'c' > c
  $ hg ci -Am 'C'
  adding c

  $ hg up -q 0
  $ echo 'd' > d
  $ hg ci -Am 'D'
  adding d

  $ hg tglog
  @  3: 'D'
  |
  | o  2: 'C' 2
  | |
  | o  1: 'B' 1
  |/
  o  0: 'A'
  
  $ hg rebase --keepbranches --collapse -s 1 -d 3
  abort: cannot collapse multiple named branches
  [255]

