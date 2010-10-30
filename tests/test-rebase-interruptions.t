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
  $ hg ci -Am A
  adding A

  $ echo B > B
  $ hg ci -Am B
  adding B

  $ echo C >> A
  $ hg ci -m C

  $ hg up -q -C 0

  $ echo D >> A
  $ hg ci -m D
  created new head

  $ echo E > E
  $ hg ci -Am E
  adding E

  $ cd ..


Changes during an interruption - continue:

  $ hg clone -q -u . a a1
  $ cd a1

  $ hg tglog
  @  4: 'E'
  |
  o  3: 'D'
  |
  | o  2: 'C'
  | |
  | o  1: 'B'
  |/
  o  0: 'A'
  
Rebasing B onto E:

  $ hg rebase -s 1 -d 4
  merging A
  warning: conflicts during merge.
  merging A failed!
  abort: unresolved conflicts (see hg resolve, then hg rebase --continue)
  [255]

Force a commit on C during the interruption:

  $ hg up -q -C 2

  $ echo 'Extra' > Extra
  $ hg add Extra
  $ hg ci -m 'Extra'

  $ hg tglog
  @  6: 'Extra'
  |
  | o  5: 'B'
  | |
  | o  4: 'E'
  | |
  | o  3: 'D'
  | |
  o |  2: 'C'
  | |
  o |  1: 'B'
  |/
  o  0: 'A'
  
Resume the rebasing:

  $ hg rebase --continue
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
  warning: new changesets detected on source branch, not stripping

  $ hg tglog
  @  7: 'C'
  |
  | o  6: 'Extra'
  | |
  o |  5: 'B'
  | |
  o |  4: 'E'
  | |
  o |  3: 'D'
  | |
  | o  2: 'C'
  | |
  | o  1: 'B'
  |/
  o  0: 'A'
  
  $ cd ..


Changes during an interruption - abort:

  $ hg clone -q -u . a a2
  $ cd a2

  $ hg tglog
  @  4: 'E'
  |
  o  3: 'D'
  |
  | o  2: 'C'
  | |
  | o  1: 'B'
  |/
  o  0: 'A'
  
Rebasing B onto E:

  $ hg rebase -s 1 -d 4
  merging A
  warning: conflicts during merge.
  merging A failed!
  abort: unresolved conflicts (see hg resolve, then hg rebase --continue)
  [255]

Force a commit on B' during the interruption:

  $ hg up -q -C 5

  $ echo 'Extra' > Extra
  $ hg add Extra
  $ hg ci -m 'Extra'

  $ hg tglog
  @  6: 'Extra'
  |
  o  5: 'B'
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
  
Abort the rebasing:

  $ hg rebase --abort
  warning: new changesets detected on target branch, can't abort
  [255]

  $ hg tglog
  @  6: 'Extra'
  |
  o  5: 'B'
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
  
  $ cd ..

