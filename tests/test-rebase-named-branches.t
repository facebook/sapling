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
  $ cd ..

  $ hg clone -q -u . a a1

  $ cd a1

  $ hg update 3
  3 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg branch dev-one
  marked working directory as branch dev-one
  (branches are permanent and global, did you want a bookmark?)
  $ hg ci -m 'dev-one named branch'

  $ hg update 7
  2 files updated, 0 files merged, 3 files removed, 0 files unresolved
  $ hg branch dev-two
  marked working directory as branch dev-two
  (branches are permanent and global, did you want a bookmark?)

  $ echo x > x

  $ hg add x

  $ hg ci -m 'dev-two named branch'

  $ hg tglog
  @  9: 'dev-two named branch' dev-two
  |
  | o  8: 'dev-one named branch' dev-one
  | |
  o |  7: 'H'
  | |
  +---o  6: 'G'
  | | |
  o | |  5: 'F'
  | | |
  +---o  4: 'E'
  | |
  | o  3: 'D'
  | |
  | o  2: 'C'
  | |
  | o  1: 'B'
  |/
  o  0: 'A'
  

Branch name containing a dash (issue3181)

  $ hg rebase -b dev-two -d dev-one --keepbranches
  saved backup bundle to $TESTTMP/a1/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  9: 'dev-two named branch' dev-two
  |
  o  8: 'H'
  |
  | o  7: 'G'
  |/|
  o |  6: 'F'
  | |
  o |  5: 'dev-one named branch' dev-one
  | |
  | o  4: 'E'
  | |
  o |  3: 'D'
  | |
  o |  2: 'C'
  | |
  o |  1: 'B'
  |/
  o  0: 'A'
  
  $ hg rebase -s dev-one -d 0 --keepbranches
  saved backup bundle to $TESTTMP/a1/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  8: 'dev-two named branch' dev-two
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
  
  $ hg update 3
  3 files updated, 0 files merged, 3 files removed, 0 files unresolved
  $ hg branch dev-one
  marked working directory as branch dev-one
  (branches are permanent and global, did you want a bookmark?)
  $ hg ci -m 'dev-one named branch'

  $ hg tglog
  @  9: 'dev-one named branch' dev-one
  |
  | o  8: 'dev-two named branch' dev-two
  | |
  | o  7: 'H'
  | |
  | | o  6: 'G'
  | |/|
  | o |  5: 'F'
  | | |
  | | o  4: 'E'
  | |/
  o |  3: 'D'
  | |
  o |  2: 'C'
  | |
  o |  1: 'B'
  |/
  o  0: 'A'
  
  $ hg rebase -b 'max(branch("dev-two"))' -d dev-one --keepbranches
  saved backup bundle to $TESTTMP/a1/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  o  9: 'dev-two named branch' dev-two
  |
  o  8: 'H'
  |
  | o  7: 'G'
  |/|
  o |  6: 'F'
  | |
  @ |  5: 'dev-one named branch' dev-one
  | |
  | o  4: 'E'
  | |
  o |  3: 'D'
  | |
  o |  2: 'C'
  | |
  o |  1: 'B'
  |/
  o  0: 'A'
  
  $ hg rebase -s 'max(branch("dev-one"))' -d 0 --keepbranches
  saved backup bundle to $TESTTMP/a1/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  o  8: 'dev-two named branch' dev-two
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
  @  0: 'A'
  

Rebasing descendant onto ancestor across different named branches

  $ hg rebase -s 1 -d 8 --keepbranches
  saved backup bundle to $TESTTMP/a1/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  o  8: 'D'
  |
  o  7: 'C'
  |
  o  6: 'B'
  |
  o  5: 'dev-two named branch' dev-two
  |
  o  4: 'H'
  |
  | o  3: 'G'
  |/|
  o |  2: 'F'
  | |
  | o  1: 'E'
  |/
  @  0: 'A'
  
  $ hg rebase -s 4 -d 5
  abort: source is ancestor of destination
  [255]

  $ hg rebase -s 5 -d 4
  saved backup bundle to $TESTTMP/a1/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  o  8: 'D'
  |
  o  7: 'C'
  |
  o  6: 'B'
  |
  o  5: 'dev-two named branch'
  |
  o  4: 'H'
  |
  | o  3: 'G'
  |/|
  o |  2: 'F'
  | |
  | o  1: 'E'
  |/
  @  0: 'A'
  
  $ cd ..
