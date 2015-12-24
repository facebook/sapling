  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > rebase=
  > 
  > [phases]
  > publish=False
  > 
  > [alias]
  > tglog = log -G --template "{rev}:{phase} '{desc}' {branches}\n"
  > EOF


  $ hg init a
  $ cd a

  $ echo A > A
  $ hg add A
  $ hg ci -m A

  $ echo 'B' > B
  $ hg add B
  $ hg ci -m B

  $ echo C >> A
  $ hg ci -m C

  $ hg up -q -C 0

  $ echo D >> A
  $ hg ci -m D
  created new head

  $ echo E > E
  $ hg add E
  $ hg ci -m E

  $ hg up -q -C 0

  $ hg branch 'notdefault'
  marked working directory as branch notdefault
  (branches are permanent and global, did you want a bookmark?)
  $ echo F >> A
  $ hg ci -m F

  $ cd ..


Rebasing B onto E - check keep: and phases

  $ hg clone -q -u . a a1
  $ cd a1
  $ hg phase --force --secret 2

  $ hg tglog
  @  5:draft 'F' notdefault
  |
  | o  4:draft 'E'
  | |
  | o  3:draft 'D'
  |/
  | o  2:secret 'C'
  | |
  | o  1:draft 'B'
  |/
  o  0:draft 'A'
  
  $ hg rebase -s 1 -d 4 --keep
  rebasing 1:27547f69f254 "B"
  rebasing 2:965c486023db "C"
  merging A
  warning: conflicts while merging A! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

Solve the conflict and go on:

  $ echo 'conflict solved' > A
  $ rm A.orig
  $ hg resolve -m A
  (no more unresolved files)
  continue: hg rebase --continue
  $ hg rebase --continue
  already rebased 1:27547f69f254 "B" as 45396c49d53b
  rebasing 2:965c486023db "C"

  $ hg tglog
  o  7:secret 'C'
  |
  o  6:draft 'B'
  |
  | @  5:draft 'F' notdefault
  | |
  o |  4:draft 'E'
  | |
  o |  3:draft 'D'
  |/
  | o  2:secret 'C'
  | |
  | o  1:draft 'B'
  |/
  o  0:draft 'A'
  
  $ cd ..


Rebase F onto E - check keepbranches:

  $ hg clone -q -u . a a2
  $ cd a2
  $ hg phase --force --secret 2

  $ hg tglog
  @  5:draft 'F' notdefault
  |
  | o  4:draft 'E'
  | |
  | o  3:draft 'D'
  |/
  | o  2:secret 'C'
  | |
  | o  1:draft 'B'
  |/
  o  0:draft 'A'
  
  $ hg rebase -s 5 -d 4 --keepbranches
  rebasing 5:01e6ebbd8272 "F" (tip)
  merging A
  warning: conflicts while merging A! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

Solve the conflict and go on:

  $ echo 'conflict solved' > A
  $ rm A.orig
  $ hg resolve -m A
  (no more unresolved files)
  continue: hg rebase --continue
  $ hg rebase --continue
  rebasing 5:01e6ebbd8272 "F" (tip)
  saved backup bundle to $TESTTMP/a2/.hg/strip-backup/01e6ebbd8272-6fd3a015-backup.hg (glob)

  $ hg tglog
  @  5:draft 'F' notdefault
  |
  o  4:draft 'E'
  |
  o  3:draft 'D'
  |
  | o  2:secret 'C'
  | |
  | o  1:draft 'B'
  |/
  o  0:draft 'A'
  

  $ cd ..
