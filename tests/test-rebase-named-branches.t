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


Rebasing descendant onto ancestor across different named branches

  $ hg clone -q -u . a a1

  $ cd a1

  $ hg branch dev
  marked working directory as branch dev

  $ echo x > x

  $ hg add x

  $ hg ci -m 'extra named branch'

  $ hg tglog
  @  8: 'extra named branch' dev
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
  


  $ hg rebase -s 1 -d 8 --keepbranches
  saved backup bundle to $TESTTMP/a1/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  8: 'D'
  |
  o  7: 'C'
  |
  o  6: 'B'
  |
  o  5: 'extra named branch' dev
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
  
  $ hg rebase -s 4 -d 5
  abort: source is ancestor of destination
  [255]

  $ hg rebase -s 5 -d 4
  saved backup bundle to $TESTTMP/a1/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  8: 'D'
  |
  o  7: 'C'
  |
  o  6: 'B'
  |
  o  5: 'extra named branch'
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
