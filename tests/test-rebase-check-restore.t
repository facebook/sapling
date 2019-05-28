  $ setconfig extensions.treemanifest=!
TODO: Make this test compatibile with obsstore enabled.
  $ setconfig experimental.evolution=
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > rebase=
  > 
  > [phases]
  > publish=False
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

  $ echo E > E
  $ hg add E
  $ hg ci -m E

  $ hg up -q -C 0

  $ echo F >> A
  $ hg ci -m F

  $ cd ..


Rebasing B onto E - check keep: and phases

  $ hg clone -q -u . a a1
  $ cd a1
  $ hg phase --force --secret 2

  $ tglogp
  @  5: 3225f3ea730a draft 'F'
  |
  | o  4: ae36e8e3dfd7 draft 'E'
  | |
  | o  3: 46b37eabc604 draft 'D'
  |/
  | o  2: 965c486023db secret 'C'
  | |
  | o  1: 27547f69f254 draft 'B'
  |/
  o  0: 4a2df7238c3b draft 'A'
  
  $ hg rebase -s 1 -d 4 --keep
  rebasing 1:27547f69f254 "B"
  rebasing 2:965c486023db "C"
  merging A
  warning: 1 conflicts while merging A! (edit, then use 'hg resolve --mark')
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

  $ tglogp
  o  7: d2d25e26288e secret 'C'
  |
  o  6: 45396c49d53b draft 'B'
  |
  | @  5: 3225f3ea730a draft 'F'
  | |
  o |  4: ae36e8e3dfd7 draft 'E'
  | |
  o |  3: 46b37eabc604 draft 'D'
  |/
  | o  2: 965c486023db secret 'C'
  | |
  | o  1: 27547f69f254 draft 'B'
  |/
  o  0: 4a2df7238c3b draft 'A'
  
  $ cd ..


Rebase F onto E:

  $ hg clone -q -u . a a2
  $ cd a2
  $ hg phase --force --secret 2

  $ tglogp
  @  5: 3225f3ea730a draft 'F'
  |
  | o  4: ae36e8e3dfd7 draft 'E'
  | |
  | o  3: 46b37eabc604 draft 'D'
  |/
  | o  2: 965c486023db secret 'C'
  | |
  | o  1: 27547f69f254 draft 'B'
  |/
  o  0: 4a2df7238c3b draft 'A'
  
  $ hg rebase -s 5 -d 4
  rebasing 5:3225f3ea730a "F" (tip)
  merging A
  warning: 1 conflicts while merging A! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

Solve the conflict and go on:

  $ echo 'conflict solved' > A
  $ rm A.orig
  $ hg resolve -m A
  (no more unresolved files)
  continue: hg rebase --continue
  $ hg rebase --continue
  rebasing 5:3225f3ea730a "F" (tip)
  saved backup bundle to $TESTTMP/a2/.hg/strip-backup/3225f3ea730a-289ce185-rebase.hg

  $ tglogp
  @  5: 530bc6058bd0 draft 'F'
  |
  o  4: ae36e8e3dfd7 draft 'E'
  |
  o  3: 46b37eabc604 draft 'D'
  |
  | o  2: 965c486023db secret 'C'
  | |
  | o  1: 27547f69f254 draft 'B'
  |/
  o  0: 4a2df7238c3b draft 'A'
  

  $ cd ..
