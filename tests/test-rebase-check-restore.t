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
  $ hg add A
  $ hg ci -m A

  $ echo 'B' > B
  $ hg add B
  $ hg ci -m B

  $ echo C >> A
  $ hg ci -m C

  $ hg up -q -C 0

  $ echo D >> A
  $ hg ci -m D
  created new head

  $ echo E > E
  $ hg add E
  $ hg ci -m E

  $ hg up -q -C 0

  $ hg branch 'notdefault'
  marked working directory as branch notdefault
  $ echo F >> A
  $ hg ci -m F

  $ cd ..


Rebasing B onto E - check keep:

  $ hg clone -q -u . a a1
  $ cd a1

  $ hg tglog
  @  5: 'F' notdefault
  |
  | o  4: 'E'
  | |
  | o  3: 'D'
  |/
  | o  2: 'C'
  | |
  | o  1: 'B'
  |/
  o  0: 'A'
  
  $ hg rebase -s 1 -d 4 --keep
  merging A
  warning: conflicts during merge.
  merging A failed!
  abort: unresolved conflicts (see hg resolve, then hg rebase --continue)
  [255]

Solve the conflict and go on:

  $ echo 'conflict solved' > A
  $ rm A.orig
  $ hg resolve -m A
  $ hg rebase --continue

  $ hg tglog
  @  7: 'C'
  |
  o  6: 'B'
  |
  | o  5: 'F' notdefault
  | |
  o |  4: 'E'
  | |
  o |  3: 'D'
  |/
  | o  2: 'C'
  | |
  | o  1: 'B'
  |/
  o  0: 'A'
  
  $ cd ..


Rebase F onto E - check keepbranches:

  $ hg clone -q -u . a a2
  $ cd a2

  $ hg tglog
  @  5: 'F' notdefault
  |
  | o  4: 'E'
  | |
  | o  3: 'D'
  |/
  | o  2: 'C'
  | |
  | o  1: 'B'
  |/
  o  0: 'A'
  
  $ hg rebase -s 5 -d 4 --keepbranches
  merging A
  warning: conflicts during merge.
  merging A failed!
  abort: unresolved conflicts (see hg resolve, then hg rebase --continue)
  [255]

Solve the conflict and go on:

  $ echo 'conflict solved' > A
  $ rm A.orig
  $ hg resolve -m A
  $ hg rebase --continue
  saved backup bundle to $TESTTMP/a2/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  5: 'F' notdefault
  |
  o  4: 'E'
  |
  o  3: 'D'
  |
  | o  2: 'C'
  | |
  | o  1: 'B'
  |/
  o  0: 'A'
  
