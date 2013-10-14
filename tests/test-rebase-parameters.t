  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > graphlog=
  > rebase=
  > 
  > [phases]
  > publish=False
  > 
  > [alias]
  > tglog = log -G --template "{rev}: '{desc}' {branches}\n"
  > EOF


  $ hg init a
  $ cd a
  $ hg unbundle "$TESTDIR/bundles/rebase.hg"
  adding changesets
  adding manifests
  adding file changes
  added 8 changesets with 7 changes to 7 files (+2 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hg up tip
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ echo I > I
  $ hg ci -AmI
  adding I

  $ hg tglog
  @  8: 'I'
  |
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
  
  $ cd ..


These fail:

  $ hg clone -q -u . a a1
  $ cd a1

  $ hg rebase -s 8 -d 7
  nothing to rebase
  [1]

  $ hg rebase --continue --abort
  abort: cannot use both abort and continue
  [255]

  $ hg rebase --continue --collapse
  abort: cannot use collapse with continue or abort
  [255]

  $ hg rebase --continue --dest 4
  abort: abort and continue do not allow specifying revisions
  [255]

  $ hg rebase --base 5 --source 4
  abort: cannot specify both a source and a base
  [255]

  $ hg rebase --rev 5 --source 4
  abort: cannot specify both a revision and a source
  [255]
  $ hg rebase --base 5 --rev 4
  abort: cannot specify both a revision and a base
  [255]

  $ hg rebase
  nothing to rebase
  [1]

  $ hg up -q 7

  $ hg rebase --traceback
  nothing to rebase
  [1]


These work:

Rebase with no arguments (from 3 onto 8):

  $ hg up -q -C 3

  $ hg rebase
  saved backup bundle to $TESTTMP/a1/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  8: 'D'
  |
  o  7: 'C'
  |
  o  6: 'B'
  |
  o  5: 'I'
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
  
Try to rollback after a rebase (fail):

  $ hg rollback
  no rollback information available
  [1]

  $ cd ..


Rebase with base == '.' => same as no arguments (from 3 onto 8):

  $ hg clone -q -u 3 a a2
  $ cd a2

  $ hg rebase --base .
  saved backup bundle to $TESTTMP/a2/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  8: 'D'
  |
  o  7: 'C'
  |
  o  6: 'B'
  |
  o  5: 'I'
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
  
  $ cd ..


Rebase with dest == branch(.) => same as no arguments (from 3 onto 8):

  $ hg clone -q -u 3 a a3
  $ cd a3

  $ hg rebase --dest 'branch(.)'
  saved backup bundle to $TESTTMP/a3/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  8: 'D'
  |
  o  7: 'C'
  |
  o  6: 'B'
  |
  o  5: 'I'
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
  
  $ cd ..


Specify only source (from 2 onto 8):

  $ hg clone -q -u . a a4
  $ cd a4

  $ hg rebase --source 'desc("C")'
  saved backup bundle to $TESTTMP/a4/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  o  8: 'D'
  |
  o  7: 'C'
  |
  @  6: 'I'
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
  
  $ cd ..


Specify only dest (from 3 onto 6):

  $ hg clone -q -u 3 a a5
  $ cd a5

  $ hg rebase --dest 6
  saved backup bundle to $TESTTMP/a5/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  8: 'D'
  |
  o  7: 'C'
  |
  o  6: 'B'
  |
  | o  5: 'I'
  | |
  | o  4: 'H'
  | |
  o |  3: 'G'
  |\|
  | o  2: 'F'
  | |
  o |  1: 'E'
  |/
  o  0: 'A'
  
  $ cd ..


Specify only base (from 1 onto 8):

  $ hg clone -q -u . a a6
  $ cd a6

  $ hg rebase --base 'desc("D")'
  saved backup bundle to $TESTTMP/a6/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  o  8: 'D'
  |
  o  7: 'C'
  |
  o  6: 'B'
  |
  @  5: 'I'
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
  
  $ cd ..


Specify source and dest (from 2 onto 7):

  $ hg clone -q -u . a a7
  $ cd a7

  $ hg rebase --source 2 --dest 7
  saved backup bundle to $TESTTMP/a7/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  o  8: 'D'
  |
  o  7: 'C'
  |
  | @  6: 'I'
  |/
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
  
  $ cd ..


Specify base and dest (from 1 onto 7):

  $ hg clone -q -u . a a8
  $ cd a8

  $ hg rebase --base 3 --dest 7
  saved backup bundle to $TESTTMP/a8/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  o  8: 'D'
  |
  o  7: 'C'
  |
  o  6: 'B'
  |
  | @  5: 'I'
  |/
  o  4: 'H'
  |
  | o  3: 'G'
  |/|
  o |  2: 'F'
  | |
  | o  1: 'E'
  |/
  o  0: 'A'
  
  $ cd ..


Specify only revs (from 2 onto 8)

  $ hg clone -q -u . a a9
  $ cd a9

  $ hg rebase --rev 'desc("C")::'
  saved backup bundle to $TESTTMP/a9/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  o  8: 'D'
  |
  o  7: 'C'
  |
  @  6: 'I'
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
  
  $ cd ..

Test --tool parameter:

  $ hg init b
  $ cd b

  $ echo c1 > c1
  $ hg ci -Am c1
  adding c1

  $ echo c2 > c2
  $ hg ci -Am c2
  adding c2

  $ hg up -q 0
  $ echo c2b > c2
  $ hg ci -Am c2b
  adding c2
  created new head

  $ cd ..

  $ hg clone -q -u . b b1
  $ cd b1

  $ hg rebase -s 2 -d 1 --tool internal:local
  saved backup bundle to $TESTTMP/b1/.hg/strip-backup/*-backup.hg (glob)

  $ hg cat c2
  c2

  $ cd ..


  $ hg clone -q -u . b b2
  $ cd b2

  $ hg rebase -s 2 -d 1 --tool internal:other
  saved backup bundle to $TESTTMP/b2/.hg/strip-backup/*-backup.hg (glob)

  $ hg cat c2
  c2b

  $ cd ..


  $ hg clone -q -u . b b3
  $ cd b3

  $ hg rebase -s 2 -d 1 --tool internal:fail
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

  $ hg summary
  parent: 1:56daeba07f4b 
   c2
  parent: 2:e4e3f3546619 tip
   c2b
  branch: default
  commit: 1 modified, 1 unresolved (merge)
  update: (current)
  rebase: 0 rebased, 1 remaining (rebase --continue)

  $ hg resolve -l
  U c2

  $ hg resolve -m c2
  $ hg rebase -c --tool internal:fail
  tool option will be ignored
  saved backup bundle to $TESTTMP/b3/.hg/strip-backup/*-backup.hg (glob)

  $ cd ..
