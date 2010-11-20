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

  $ echo C > C
  $ hg ci -Am C
  adding C

  $ echo D > D
  $ hg ci -Am D
  adding D

  $ hg up -q -C 0

  $ echo E > E
  $ hg ci -Am E
  adding E
  created new head

  $ cd ..


Rebasing D onto E detaching from C:

  $ hg clone -q -u . a a1
  $ cd a1

  $ hg tglog
  @  4: 'E'
  |
  | o  3: 'D'
  | |
  | o  2: 'C'
  | |
  | o  1: 'B'
  |/
  o  0: 'A'
  
  $ hg rebase --detach -s 3 -d 4
  saved backup bundle to $TESTTMP/a1/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  4: 'D'
  |
  o  3: 'E'
  |
  | o  2: 'C'
  | |
  | o  1: 'B'
  |/
  o  0: 'A'
  
  $ hg manifest
  A
  D
  E

  $ cd ..


Rebasing C onto E detaching from B:

  $ hg clone -q -u . a a2
  $ cd a2

  $ hg tglog
  @  4: 'E'
  |
  | o  3: 'D'
  | |
  | o  2: 'C'
  | |
  | o  1: 'B'
  |/
  o  0: 'A'
  
  $ hg rebase --detach -s 2 -d 4
  saved backup bundle to $TESTTMP/a2/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  4: 'D'
  |
  o  3: 'C'
  |
  o  2: 'E'
  |
  | o  1: 'B'
  |/
  o  0: 'A'
  
  $ hg manifest
  A
  C
  D
  E

  $ cd ..


Rebasing B onto E using detach (same as not using it):

  $ hg clone -q -u . a a3
  $ cd a3

  $ hg tglog
  @  4: 'E'
  |
  | o  3: 'D'
  | |
  | o  2: 'C'
  | |
  | o  1: 'B'
  |/
  o  0: 'A'
  
  $ hg rebase --detach -s 1 -d 4
  saved backup bundle to $TESTTMP/a3/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  4: 'D'
  |
  o  3: 'C'
  |
  o  2: 'B'
  |
  o  1: 'E'
  |
  o  0: 'A'
  
  $ hg manifest
  A
  B
  C
  D
  E

  $ cd ..


Rebasing C onto E detaching from B and collapsing:

  $ hg clone -q -u . a a4
  $ cd a4

  $ hg tglog
  @  4: 'E'
  |
  | o  3: 'D'
  | |
  | o  2: 'C'
  | |
  | o  1: 'B'
  |/
  o  0: 'A'
  
  $ hg rebase --detach --collapse -s 2 -d 4
  saved backup bundle to $TESTTMP/a4/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  3: 'Collapsed revision
  |  * C
  |  * D'
  o  2: 'E'
  |
  | o  1: 'B'
  |/
  o  0: 'A'
  
  $ hg manifest
  A
  C
  D
  E

  $ cd ..

Rebasing across null as ancestor
  $ hg clone -q -U a a5

  $ cd a5

  $ echo x > x

  $ hg add x

  $ hg ci -m "extra branch"
  created new head

  $ hg tglog
  @  5: 'extra branch'
  
  o  4: 'E'
  |
  | o  3: 'D'
  | |
  | o  2: 'C'
  | |
  | o  1: 'B'
  |/
  o  0: 'A'
  
  $ hg rebase --detach -s 1 -d tip
  saved backup bundle to $TESTTMP/a5/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  5: 'D'
  |
  o  4: 'C'
  |
  o  3: 'B'
  |
  o  2: 'extra branch'
  
  o  1: 'E'
  |
  o  0: 'A'
  
