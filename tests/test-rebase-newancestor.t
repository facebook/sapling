  $ cat >> $HGRCPATH <<EOF
  > [extensions]
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


Test rebasing of merges with ancestors of the rebase destination - a situation
that often happens when trying to recover from repeated merging with a mainline
branch.

The test case creates a dev branch that contains a couple of merges from the
default branch. When rebasing to the default branch, these merges would be
merges with ancestors on the same branch. The merges _could_ contain some
interesting conflict resolutions or additional changes in the merge commit, but
that is mixed up with the actual merge stuff and there is in general no way to
separate them.

Note: The dev branch contains _no_ changes to f-default. It might be unclear
how rebasing of ancestor merges should be handled, but the current behavior
with spurious prompts for conflicts in files that didn't change seems very
wrong.

  $ hg init ancestor-merge
  $ cd ancestor-merge

  $ touch f-default
  $ hg ci -Aqm 'default: create f-default'

  $ hg branch -q dev
  $ hg ci -qm 'dev: create branch'

  $ echo stuff > f-dev
  $ hg ci -Aqm 'dev: f-dev stuff'

  $ hg up -q default
  $ echo stuff > f-default
  $ hg ci -m 'default: f-default stuff'

  $ hg up -q dev
  $ hg merge -q default
  $ hg ci -m 'dev: merge default'

  $ hg up -q default
  $ hg rm f-default
  $ hg ci -m 'default: remove f-default'

  $ hg up -q dev
  $ hg merge -q default
  $ hg ci -m 'dev: merge default'

  $ hg up -q default
  $ echo stuff > f-other
  $ hg ci -Aqm 'default: f-other stuff'

  $ hg tglog
  @  7: 'default: f-other stuff'
  |
  | o  6: 'dev: merge default' dev
  |/|
  o |  5: 'default: remove f-default'
  | |
  | o  4: 'dev: merge default' dev
  |/|
  o |  3: 'default: f-default stuff'
  | |
  | o  2: 'dev: f-dev stuff' dev
  | |
  | o  1: 'dev: create branch' dev
  |/
  o  0: 'default: create f-default'
  
  $ hg clone -qU . ../ancestor-merge-2

Full rebase all the way back from branching point:

  $ hg rebase -r 'only(dev,default)' -d default
  remote changed f-default which local deleted
  use (c)hanged version or leave (d)eleted? c
  local changed f-default which remote deleted
  use (c)hanged version or (d)elete? c
  saved backup bundle to $TESTTMP/ancestor-merge/.hg/strip-backup/1d1a643d390e-backup.hg (glob)
  $ hg tglog
  o  5: 'dev: merge default'
  |
  o  4: 'dev: f-dev stuff'
  |
  @  3: 'default: f-other stuff'
  |
  o  2: 'default: remove f-default'
  |
  o  1: 'default: f-default stuff'
  |
  o  0: 'default: create f-default'
  
Grafty cherry picking rebasing:

  $ cd ../ancestor-merge-2

  $ hg phase -fdr0:
  $ hg rebase -r 'children(only(dev,default))' -d default
  remote changed f-default which local deleted
  use (c)hanged version or leave (d)eleted? c
  local changed f-default which remote deleted
  use (c)hanged version or (d)elete? c
  saved backup bundle to $TESTTMP/ancestor-merge-2/.hg/strip-backup/ec2c14fb2984-backup.hg (glob)
  $ hg tglog
  o  6: 'dev: merge default'
  |
  o  5: 'dev: f-dev stuff'
  |
  o  4: 'default: f-other stuff'
  |
  o  3: 'default: remove f-default'
  |
  o  2: 'default: f-default stuff'
  |
  | o  1: 'dev: create branch' dev
  |/
  o  0: 'default: create f-default'
  
