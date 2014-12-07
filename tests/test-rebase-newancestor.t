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
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/0f4f7cb4f549-backup.hg (glob)

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
  saved backup bundle to $TESTTMP/ancestor-merge/.hg/strip-backup/1d1a643d390e-backup.hg (glob)
  $ hg tglog
  o  6: 'dev: merge default'
  |
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
  saved backup bundle to $TESTTMP/ancestor-merge-2/.hg/strip-backup/ec2c14fb2984-backup.hg (glob)
  $ hg tglog
  o  7: 'dev: merge default'
  |
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
  
  $ cd ..


Test order of parents of rebased merged with un-rebased changes as p1.

  $ hg init parentorder
  $ cd parentorder
  $ touch f
  $ hg ci -Aqm common
  $ touch change
  $ hg ci -Aqm change
  $ touch target
  $ hg ci -Aqm target
  $ hg up -qr 0
  $ touch outside
  $ hg ci -Aqm outside
  $ hg merge -qr 1
  $ hg ci -m 'merge p1 3=outside p2 1=ancestor'
  $ hg par
  changeset:   4:6990226659be
  tag:         tip
  parent:      3:f59da8fc0fcf
  parent:      1:dd40c13f7a6f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     merge p1 3=outside p2 1=ancestor
  
  $ hg up -qr 1
  $ hg merge -qr 3
  $ hg ci -qm 'merge p1 1=ancestor p2 3=outside'
  $ hg par
  changeset:   5:a57575f79074
  tag:         tip
  parent:      1:dd40c13f7a6f
  parent:      3:f59da8fc0fcf
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     merge p1 1=ancestor p2 3=outside
  
  $ hg tglog
  @    5: 'merge p1 1=ancestor p2 3=outside'
  |\
  +---o  4: 'merge p1 3=outside p2 1=ancestor'
  | |/
  | o  3: 'outside'
  | |
  +---o  2: 'target'
  | |
  o |  1: 'change'
  |/
  o  0: 'common'
  
  $ hg rebase -r 4 -d 2
  saved backup bundle to $TESTTMP/parentorder/.hg/strip-backup/6990226659be-backup.hg (glob)
  $ hg tip
  changeset:   5:cca50676b1c5
  tag:         tip
  parent:      2:a60552eb93fb
  parent:      3:f59da8fc0fcf
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     merge p1 3=outside p2 1=ancestor
  
  $ hg rebase -r 4 -d 2
  saved backup bundle to $TESTTMP/parentorder/.hg/strip-backup/a57575f79074-backup.hg (glob)
  $ hg tip
  changeset:   5:f9daf77ffe76
  tag:         tip
  parent:      2:a60552eb93fb
  parent:      3:f59da8fc0fcf
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     merge p1 1=ancestor p2 3=outside
  
  $ hg tglog
  @    5: 'merge p1 1=ancestor p2 3=outside'
  |\
  +---o  4: 'merge p1 3=outside p2 1=ancestor'
  | |/
  | o  3: 'outside'
  | |
  o |  2: 'target'
  | |
  o |  1: 'change'
  |/
  o  0: 'common'
  
rebase of merge of ancestors

  $ hg up -qr 2
  $ hg merge -qr 3
  $ echo 'other change while merging future "rebase ancestors"' > other
  $ hg ci -Aqm 'merge rebase ancestors'
  $ hg rebase -d 5 -v
  resolving manifests
  removing other
  note: merging f9daf77ffe76+ and 4c5f12f25ebe using bids from ancestors a60552eb93fb and f59da8fc0fcf
  
  calculating bids for ancestor a60552eb93fb
  resolving manifests
  
  calculating bids for ancestor f59da8fc0fcf
  resolving manifests
  
  auction for merging merge bids
   other: consensus for g
  end of auction
  
  getting other
  other
  rebase merging completed
  1 changesets found
  saved backup bundle to $TESTTMP/parentorder/.hg/strip-backup/4c5f12f25ebe-backup.hg (glob)
  1 changesets found
  adding branch
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  rebase completed
  $ hg tglog
  @  6: 'merge rebase ancestors'
  |
  o    5: 'merge p1 1=ancestor p2 3=outside'
  |\
  +---o  4: 'merge p1 3=outside p2 1=ancestor'
  | |/
  | o  3: 'outside'
  | |
  o |  2: 'target'
  | |
  o |  1: 'change'
  |/
  o  0: 'common'
  
