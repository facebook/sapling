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

  $ echo l2 > l2
  $ hg ci -Am l2
  adding l2

  $ hg up -q -C 1

  $ hg branch 'notdefault'
  marked working directory as branch notdefault

  $ echo r1 > r1
  $ hg ci -Am r1
  adding r1

  $ hg tglog
  @  4: 'r1' notdefault
  |
  | o  3: 'l2'
  | |
  | o  2: 'l1'
  |/
  o  1: 'c2'
  |
  o  0: 'c1'
  

Rebase a branch while preserving the branch name:

  $ hg up -q -C 3

  $ hg rebase -b 4 -d 3 --keepbranches
  saved backup bundle to $TESTTMP/a/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  4: 'r1' notdefault
  |
  o  3: 'l2'
  |
  o  2: 'l1'
  |
  o  1: 'c2'
  |
  o  0: 'c1'
  
  $ hg branch
  notdefault

