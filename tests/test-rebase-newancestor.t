  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > graphlog=
  > rebase=
  > 
  > [alias]
  > tglog = log -G --template "{rev}: '{desc}' {branches}\n"
  > EOF

  $ hg init repo
  $ cd repo

  $ echo A > a
  $ echo >> a
  $ hg ci -Am A
  adding a

  $ echo B > a
  $ echo >> a
  $ hg ci -m B

  $ echo C > a
  $ echo >> a
  $ hg ci -m C

  $ hg up -q -C 0

  $ echo D >> a
  $ hg ci -Am AD
  created new head

  $ hg tglog
  @  3: 'AD'
  |
  | o  2: 'C'
  | |
  | o  1: 'B'
  |/
  o  0: 'A'
  
  $ hg rebase -s 1 -d 3
  merging a
  merging a
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  o  3: 'C'
  |
  o  2: 'B'
  |
  @  1: 'AD'
  |
  o  0: 'A'
  

  $ cd ..
