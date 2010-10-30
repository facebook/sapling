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

  $ echo c1 > common
  $ hg add common
  $ hg ci -m C1

  $ echo c2 >> common
  $ hg ci -m C2

  $ echo c3 >> common
  $ hg ci -m C3

  $ hg up -q -C 1

  $ echo l1 >> extra
  $ hg add extra
  $ hg ci -m L1
  created new head

  $ sed -e 's/c2/l2/' common > common.new
  $ mv common.new common
  $ hg ci -m L2

  $ hg tglog
  @  4: 'L2'
  |
  o  3: 'L1'
  |
  | o  2: 'C3'
  |/
  o  1: 'C2'
  |
  o  0: 'C1'
  

Conflicting rebase:

  $ hg rebase -s 3 -d 2
  merging common
  warning: conflicts during merge.
  merging common failed!
  abort: unresolved conflicts (see hg resolve, then hg rebase --continue)
  [255]

Abort:

  $ hg rebase --abort
  saved backup bundle to $TESTTMP/a/.hg/strip-backup/*-backup.hg (glob)
  rebase aborted

  $ hg tglog
  @  4: 'L2'
  |
  o  3: 'L1'
  |
  | o  2: 'C3'
  |/
  o  1: 'C2'
  |
  o  0: 'C1'
  
  $ cd ..


Constrcut new repo:

  $ hg init b
  $ cd b

  $ echo a > a
  $ hg ci -Am A
  adding a

  $ echo b > b
  $ hg ci -Am B
  adding b

  $ echo c > c
  $ hg ci -Am C
  adding c

  $ hg up -q 0

  $ echo b > b
  $ hg ci -Am 'B bis'
  adding b
  created new head

  $ echo c1 > c
  $ hg ci -Am C1
  adding c

Rebase and abort without generating new changesets:

  $ hg tglog
  @  4: 'C1'
  |
  o  3: 'B bis'
  |
  | o  2: 'C'
  | |
  | o  1: 'B'
  |/
  o  0: 'A'
  
  $ hg rebase -b 4 -d 2
  merging c
  warning: conflicts during merge.
  merging c failed!
  abort: unresolved conflicts (see hg resolve, then hg rebase --continue)
  [255]

  $ hg tglog
  @  4: 'C1'
  |
  o  3: 'B bis'
  |
  | @  2: 'C'
  | |
  | o  1: 'B'
  |/
  o  0: 'A'
  
  $ hg rebase -a
  rebase aborted

  $ hg tglog
  @  4: 'C1'
  |
  o  3: 'B bis'
  |
  | o  2: 'C'
  | |
  | o  1: 'B'
  |/
  o  0: 'A'
  
