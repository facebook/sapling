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


Rebasing
D onto H - simple rebase:

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
  

  $ hg rebase -s 3 -d 7
  saved backup bundle to $TESTTMP/a1/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @    7: 'D'
  |\
  | o  6: 'H'
  | |
  | | o  5: 'G'
  | |/|
  | o |  4: 'F'
  | | |
  | | o  3: 'E'
  | |/
  o |  2: 'C'
  | |
  o |  1: 'B'
  |/
  o  0: 'A'
  
  $ cd ..


D onto F - intermediate point:

  $ hg clone -q -u . a a2
  $ cd a2

  $ hg rebase -s 3 -d 5
  saved backup bundle to $TESTTMP/a2/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @    7: 'D'
  |\
  | | o  6: 'H'
  | |/
  | | o  5: 'G'
  | |/|
  | o |  4: 'F'
  | | |
  | | o  3: 'E'
  | |/
  o |  2: 'C'
  | |
  o |  1: 'B'
  |/
  o  0: 'A'
  
  $ cd ..


E onto H - skip of G:

  $ hg clone -q -u . a a3
  $ cd a3

  $ hg rebase -s 4 -d 7
  saved backup bundle to $TESTTMP/a3/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  6: 'E'
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
  
  $ cd ..


F onto E - rebase of a branching point (skip G):

  $ hg clone -q -u . a a4
  $ cd a4

  $ hg rebase -s 5 -d 4
  saved backup bundle to $TESTTMP/a4/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  6: 'H'
  |
  o  5: 'F'
  |
  o  4: 'E'
  |
  | o  3: 'D'
  | |
  | o  2: 'C'
  | |
  | o  1: 'B'
  |/
  o  0: 'A'
  
  $ cd ..


G onto H - merged revision having a parent in ancestors of target:

  $ hg clone -q -u . a a5
  $ cd a5

  $ hg rebase -s 6 -d 7
  saved backup bundle to $TESTTMP/a5/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @    7: 'G'
  |\
  | o  6: 'H'
  | |
  | o  5: 'F'
  | |
  o |  4: 'E'
  |/
  | o  3: 'D'
  | |
  | o  2: 'C'
  | |
  | o  1: 'B'
  |/
  o  0: 'A'
  
  $ cd ..


F onto B - G maintains E as parent:

  $ hg clone -q -u . a a6
  $ cd a6

  $ hg rebase -s 5 -d 1
  saved backup bundle to $TESTTMP/a6/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  7: 'H'
  |
  | o  6: 'G'
  |/|
  o |  5: 'F'
  | |
  | o  4: 'E'
  | |
  | | o  3: 'D'
  | | |
  +---o  2: 'C'
  | |
  o |  1: 'B'
  |/
  o  0: 'A'
  
  $ cd ..


These will fail (using --source):

G onto F - rebase onto an ancestor:

  $ hg clone -q -u . a a7
  $ cd a7

  $ hg rebase -s 6 -d 5
  nothing to rebase
  [1]

F onto G - rebase onto a descendant:

  $ hg rebase -s 5 -d 6
  abort: source is ancestor of destination
  [255]

G onto B - merge revision with both parents not in ancestors of target:

  $ hg rebase -s 6 -d 1
  abort: cannot use revision 6 as base, result would have 3 parents
  [255]


These will abort gracefully (using --base):

G onto G - rebase onto same changeset:

  $ hg rebase -b 6 -d 6
  nothing to rebase
  [1]

G onto F - rebase onto an ancestor:

  $ hg rebase -b 6 -d 5
  nothing to rebase
  [1]

F onto G - rebase onto a descendant:

  $ hg rebase -b 5 -d 6
  nothing to rebase
  [1]

C onto A - rebase onto an ancestor:

  $ hg rebase -d 0 -s 2
  saved backup bundle to $TESTTMP/a7/.hg/strip-backup/5fddd98957c8-backup.hg
  $ hg tglog
  @  7: 'D'
  |
  o  6: 'C'
  |
  | o  5: 'H'
  | |
  | | o  4: 'G'
  | |/|
  | o |  3: 'F'
  |/ /
  | o  2: 'E'
  |/
  | o  1: 'B'
  |/
  o  0: 'A'
  

