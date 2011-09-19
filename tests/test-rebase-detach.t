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
  added 8 changesets with 7 changes to 7 files (+2 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hg up tip
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cd ..


Rebasing D onto H detaching from C:

  $ hg clone -q -u . a a1
  $ cd a1

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
  
  $ hg rebase --detach -s 3 -d 7
  saved backup bundle to $TESTTMP/a1/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  7: 'D'
  |
  o  6: 'H'
  |
  | o  5: 'G'
  |/|
  o |  4: 'F'
  | |
  | o  3: 'E'
  |/
  | o  2: 'C'
  | |
  | o  1: 'B'
  |/
  o  0: 'A'
  
  $ hg manifest
  A
  D
  F
  H

  $ cd ..


Rebasing C onto H detaching from B:

  $ hg clone -q -u . a a2
  $ cd a2

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
  
  $ hg rebase --detach -s 2 -d 7
  saved backup bundle to $TESTTMP/a2/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  7: 'D'
  |
  o  6: 'C'
  |
  o  5: 'H'
  |
  | o  4: 'G'
  |/|
  o |  3: 'F'
  | |
  | o  2: 'E'
  |/
  | o  1: 'B'
  |/
  o  0: 'A'
  
  $ hg manifest
  A
  C
  D
  F
  H

  $ cd ..


Rebasing B onto H using detach (same as not using it):

  $ hg clone -q -u . a a3
  $ cd a3

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
  
  $ hg rebase --detach -s 1 -d 7
  saved backup bundle to $TESTTMP/a3/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  7: 'D'
  |
  o  6: 'C'
  |
  o  5: 'B'
  |
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


Rebasing C onto H detaching from B and collapsing:

  $ hg clone -q -u . a a4
  $ cd a4

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
  
  $ hg rebase --detach --collapse -s 2 -d 7
  saved backup bundle to $TESTTMP/a4/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  6: 'Collapsed revision
  |  * C
  |  * D'
  o  5: 'H'
  |
  | o  4: 'G'
  |/|
  o |  3: 'F'
  | |
  | o  2: 'E'
  |/
  | o  1: 'B'
  |/
  o  0: 'A'
  
  $ hg manifest
  A
  C
  D
  F
  H

  $ cd ..

Rebasing across null as ancestor
  $ hg clone -q -U a a5

  $ cd a5

  $ echo x > x

  $ hg add x

  $ hg ci -m "extra branch"
  created new head

  $ hg tglog
  @  8: 'extra branch'
  
  o  7: 'H'
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
  
  $ hg rebase --detach -s 1 -d tip
  saved backup bundle to $TESTTMP/a5/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  8: 'D'
  |
  o  7: 'C'
  |
  o  6: 'B'
  |
  o  5: 'extra branch'
  
  o  4: 'H'
  |
  | o  3: 'G'
  |/|
  o |  2: 'F'
  | |
  | o  1: 'E'
  |/
  o  0: 'A'
  

  $ hg rebase -d 5 -s 7
  saved backup bundle to $TESTTMP/a5/.hg/strip-backup/13547172c9c0-backup.hg
  $ hg tglog
  @  8: 'D'
  |
  o  7: 'C'
  |
  | o  6: 'B'
  |/
  o  5: 'extra branch'
  
  o  4: 'H'
  |
  | o  3: 'G'
  |/|
  o |  2: 'F'
  | |
  | o  1: 'E'
  |/
  o  0: 'A'
  
