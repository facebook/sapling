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

  $ echo l1 > l1
  $ hg ci -Am l1
  adding l1

  $ hg up -q -C 1

  $ echo r1 > r1
  $ hg ci -Am r1
  adding r1
  created new head

  $ echo r2 > r2
  $ hg ci -Am r2
  adding r2

  $ hg tglog
  @  4: 'r2'
  |
  o  3: 'r1'
  |
  | o  2: 'l1'
  |/
  o  1: 'c2'
  |
  o  0: 'c1'
  
Rebase with no arguments - single revision in source branch:

  $ hg up -q -C 2

  $ hg rebase
  saved backup bundle to $TESTTMP/a/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  4: 'l1'
  |
  o  3: 'r2'
  |
  o  2: 'r1'
  |
  o  1: 'c2'
  |
  o  0: 'c1'
  
  $ cd ..


  $ hg init b
  $ cd b

  $ echo c1 > c1
  $ hg ci -Am c1
  adding c1

  $ echo c2 > c2
  $ hg ci -Am c2
  adding c2

  $ echo l1 > l1
  $ hg ci -Am l1
  adding l1

  $ echo l2 > l2
  $ hg ci -Am l2
  adding l2

  $ hg up -q -C 1

  $ echo r1 > r1
  $ hg ci -Am r1
  adding r1
  created new head

  $ hg tglog
  @  4: 'r1'
  |
  | o  3: 'l2'
  | |
  | o  2: 'l1'
  |/
  o  1: 'c2'
  |
  o  0: 'c1'
  
Rebase with no arguments - single revision in target branch:

  $ hg up -q -C 3

  $ hg rebase
  saved backup bundle to $TESTTMP/b/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  4: 'l2'
  |
  o  3: 'l1'
  |
  o  2: 'r1'
  |
  o  1: 'c2'
  |
  o  0: 'c1'
  
