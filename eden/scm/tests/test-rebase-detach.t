#chg-compatible

TODO: configure mutation
  $ configure noevolution
  $ enable rebase

Rebasing D onto B detaching from C (one commit):

  $ hg init a1
  $ cd a1

  $ drawdag <<EOF
  > D
  > |
  > C B
  > |/
  > A
  > EOF
  $ hg phase --force --secret $D

  $ hg rebase -s $D -d $B
  rebasing e7b3f00ed42e "D"
  saved backup bundle to $TESTTMP/a1/.hg/strip-backup/e7b3f00ed42e-6f368371-rebase.hg

  $ hg log -G --template "{rev}:{phase} '{desc}' {branches}\n"
  o  3:secret 'D'
  |
  | o  2:draft 'C'
  | |
  o |  1:draft 'B'
  |/
  o  0:draft 'A'
  
  $ hg manifest --rev tip
  A
  B
  D

  $ cd ..


Rebasing D onto B detaching from C (two commits):

  $ hg init a2
  $ cd a2

  $ drawdag <<EOF
  > E
  > |
  > D
  > |
  > C B
  > |/
  > A
  > EOF

  $ hg rebase -s $D -d $B
  rebasing e7b3f00ed42e "D"
  rebasing 69a34c08022a "E"
  saved backup bundle to $TESTTMP/a2/.hg/strip-backup/e7b3f00ed42e-a2ec7cea-rebase.hg

  $ tglog
  o  4: ee79e0744528 'E'
  |
  o  3: 10530e1d72d9 'D'
  |
  | o  2: dc0947a82db8 'C'
  | |
  o |  1: 112478962961 'B'
  |/
  o  0: 426bada5c675 'A'
  
  $ hg manifest --rev tip
  A
  B
  D
  E

  $ cd ..

Rebasing C onto B using detach (same as not using it):

  $ hg init a3
  $ cd a3

  $ drawdag <<EOF
  > D
  > |
  > C B
  > |/
  > A
  > EOF

  $ hg rebase -s $C -d $B
  rebasing dc0947a82db8 "C"
  rebasing e7b3f00ed42e "D"
  saved backup bundle to $TESTTMP/a3/.hg/strip-backup/dc0947a82db8-b8481714-rebase.hg

  $ tglog
  o  3: 7375f3dbfb0f 'D'
  |
  o  2: bbfdd6cb49aa 'C'
  |
  o  1: 112478962961 'B'
  |
  o  0: 426bada5c675 'A'
  
  $ hg manifest --rev tip
  A
  B
  C
  D

  $ cd ..


Rebasing D onto B detaching from C and collapsing:

  $ hg init a4
  $ cd a4

  $ drawdag <<EOF
  > E
  > |
  > D
  > |
  > C B
  > |/
  > A
  > EOF
  $ hg phase --force --secret $E

  $ hg rebase --collapse -s $D -d $B
  rebasing e7b3f00ed42e "D"
  rebasing 69a34c08022a "E"
  saved backup bundle to $TESTTMP/a4/.hg/strip-backup/e7b3f00ed42e-a2ec7cea-rebase.hg

  $ hg  log -G --template "{rev}:{phase} '{desc}' {branches}\n"
  o  3:secret 'Collapsed revision
  |  * D
  |  * E'
  | o  2:draft 'C'
  | |
  o |  1:draft 'B'
  |/
  o  0:draft 'A'
  
  $ hg manifest --rev tip
  A
  B
  D
  E

  $ cd ..

Rebasing across null as ancestor
  $ hg init a5
  $ cd a5

  $ drawdag <<EOF
  > E
  > |
  > D
  > |
  > C
  > |
  > A B
  > EOF

  $ hg rebase -s $C -d $B
  rebasing dc0947a82db8 "C"
  rebasing e7b3f00ed42e "D"
  rebasing 69a34c08022a "E"
  saved backup bundle to $TESTTMP/a5/.hg/strip-backup/dc0947a82db8-3eefec98-rebase.hg

  $ tglog
  o  4: e3d0c70d606d 'E'
  |
  o  3: e9153d36a1af 'D'
  |
  o  2: a7ac28b870a8 'C'
  |
  o  1: fc2b737bb2e5 'B'
  
  o  0: 426bada5c675 'A'
  
  $ hg rebase -d 1 -s 3
  rebasing e9153d36a1af "D"
  rebasing e3d0c70d606d "E"
  saved backup bundle to $TESTTMP/a5/.hg/strip-backup/e9153d36a1af-db7388ed-rebase.hg
  $ tglog
  o  4: 2c24e540eccd 'E'
  |
  o  3: 73f786ed52ff 'D'
  |
  | o  2: a7ac28b870a8 'C'
  |/
  o  1: fc2b737bb2e5 'B'
  
  o  0: 426bada5c675 'A'
  
  $ cd ..

Verify that target is not selected as external rev (issue3085)

  $ hg init a6
  $ cd a6

  $ drawdag <<EOF
  > H
  > | G
  > |/|
  > F E
  > |/
  > A
  > EOF
  $ hg up -q $G

  $ echo "I" >> E
  $ hg ci -m "I"
  $ export I=$(hg log -r . -T "{node}")
  $ hg merge $H
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m "Merge"
  $ echo "J" >> F
  $ hg ci -m "J"
  $ tglog
  @  7: c6aaf0d259c0 'J'
  |
  o    6: 0cfbc7e8faaf 'Merge'
  |\
  | o  5: b92d164ad3cb 'I'
  | |
  o |  4: 4ea5b230dea3 'H'
  | |
  | o  3: c6001eacfde5 'G'
  |/|
  o |  2: 8908a377a434 'F'
  | |
  | o  1: 7fb047a69f22 'E'
  |/
  o  0: 426bada5c675 'A'
  
  $ hg rebase -s $I -d $H --collapse --config ui.merge=internal:other
  rebasing b92d164ad3cb "I"
  rebasing 0cfbc7e8faaf "Merge"
  rebasing c6aaf0d259c0 "J"
  saved backup bundle to $TESTTMP/a6/.hg/strip-backup/b92d164ad3cb-88fd7ab7-rebase.hg

  $ tglog
  @  5: 65079693dac4 'Collapsed revision
  |  * I
  |  * Merge
  |  * J'
  o  4: 4ea5b230dea3 'H'
  |
  | o  3: c6001eacfde5 'G'
  |/|
  o |  2: 8908a377a434 'F'
  | |
  | o  1: 7fb047a69f22 'E'
  |/
  o  0: 426bada5c675 'A'
  

  $ hg log --rev tip
  changeset:   5:65079693dac4
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Collapsed revision
  

  $ cd ..

Ensure --continue restores a correct state (issue3046) and phase:
  $ hg init a7
  $ cd a7

  $ drawdag <<EOF
  > C B
  > |/
  > A
  > EOF
  $ hg up -q $C
  $ echo 'B2' > B
  $ hg ci -A -m 'B2'
  adding B
  $ hg phase --force --secret .
  $ hg rebase -s . -d $B --config ui.merge=internal:fail
  rebasing 17b4880d2402 "B2"
  merging B
  warning: 1 conflicts while merging B! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ hg resolve --all -t internal:local
  (no more unresolved files)
  continue: hg rebase --continue
  $ hg rebase -c
  rebasing 17b4880d2402 "B2"
  note: rebase of 3:17b4880d2402 created no changes to commit
  saved backup bundle to $TESTTMP/a7/.hg/strip-backup/17b4880d2402-1ae1f6cc-rebase.hg
  $ hg  log -G --template "{rev}:{phase} '{desc}' {branches}\n"
  o  2:draft 'C'
  |
  | @  1:draft 'B'
  |/
  o  0:draft 'A'
  

  $ cd ..
