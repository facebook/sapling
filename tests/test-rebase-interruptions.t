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
  > tglogp = log -G --template "{rev}:{phase} '{desc}' {branches}\n"
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
  merging A incomplete! (edit conflicts, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

Force a commit on C during the interruption:

  $ hg up -q -C 2 --config 'extensions.rebase=!'

  $ echo 'Extra' > Extra
  $ hg add Extra
  $ hg ci -m 'Extra' --config 'extensions.rebase=!'

Force this commit onto secret phase

  $ hg phase --force --secret 6

  $ hg tglogp
  @  6:secret 'Extra'
  |
  | o  5:draft 'B'
  | |
  | o  4:draft 'E'
  | |
  | o  3:draft 'D'
  | |
  o |  2:draft 'C'
  | |
  o |  1:draft 'B'
  |/
  o  0:draft 'A'
  
Resume the rebasing:

  $ hg rebase --continue
  merging A
  warning: conflicts during merge.
  merging A incomplete! (edit conflicts, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

Solve the conflict and go on:

  $ echo 'conflict solved' > A
  $ rm A.orig
  $ hg resolve -m A

  $ hg rebase --continue
  warning: new changesets detected on source branch, not stripping

  $ hg tglogp
  o  7:draft 'C'
  |
  | o  6:secret 'Extra'
  | |
  o |  5:draft 'B'
  | |
  @ |  4:draft 'E'
  | |
  o |  3:draft 'D'
  | |
  | o  2:draft 'C'
  | |
  | o  1:draft 'B'
  |/
  o  0:draft 'A'
  
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
  merging A incomplete! (edit conflicts, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

Force a commit on B' during the interruption:

  $ hg up -q -C 5 --config 'extensions.rebase=!'

  $ echo 'Extra' > Extra
  $ hg add Extra
  $ hg ci -m 'Extra' --config 'extensions.rebase=!'

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
  warning: new changesets detected on target branch, can't strip
  rebase aborted

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

Changes during an interruption - abort (again):

  $ hg clone -q -u . a a3
  $ cd a3

  $ hg tglogp
  @  4:draft 'E'
  |
  o  3:draft 'D'
  |
  | o  2:draft 'C'
  | |
  | o  1:draft 'B'
  |/
  o  0:draft 'A'
  
Rebasing B onto E:

  $ hg rebase -s 1 -d 4
  merging A
  warning: conflicts during merge.
  merging A incomplete! (edit conflicts, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

Change phase on B and B'

  $ hg up -q -C 5 --config 'extensions.rebase=!'
  $ hg phase --public 1
  $ hg phase --public 5
  $ hg phase --secret -f 2

  $ hg tglogp
  @  5:public 'B'
  |
  o  4:public 'E'
  |
  o  3:public 'D'
  |
  | o  2:secret 'C'
  | |
  | o  1:public 'B'
  |/
  o  0:public 'A'
  
Abort the rebasing:

  $ hg rebase --abort
  warning: can't clean up immutable changesets 45396c49d53b
  rebase aborted

  $ hg tglogp
  @  5:public 'B'
  |
  o  4:public 'E'
  |
  o  3:public 'D'
  |
  | o  2:secret 'C'
  | |
  | o  1:public 'B'
  |/
  o  0:public 'A'
  
  $ cd ..
