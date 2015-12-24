  $ cat >> $HGRCPATH <<EOF
  > [extensions]
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


Rebasing D onto H detaching from C:

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
  
  $ hg phase --force --secret 3
  $ hg rebase -s 3 -d 7
  rebasing 3:32af7686d403 "D"
  saved backup bundle to $TESTTMP/a1/.hg/strip-backup/32af7686d403-6f7dface-backup.hg (glob)

  $ hg log -G --template "{rev}:{phase} '{desc}' {branches}\n"
  o  7:secret 'D'
  |
  @  6:draft 'H'
  |
  | o  5:draft 'G'
  |/|
  o |  4:draft 'F'
  | |
  | o  3:draft 'E'
  |/
  | o  2:draft 'C'
  | |
  | o  1:draft 'B'
  |/
  o  0:draft 'A'
  
  $ hg manifest --rev tip
  A
  D
  F
  H

  $ cd ..


Rebasing C onto H detaching from B:

  $ hg clone -q -u . a a2
  $ cd a2

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
  
  $ hg rebase -s 2 -d 7
  rebasing 2:5fddd98957c8 "C"
  rebasing 3:32af7686d403 "D"
  saved backup bundle to $TESTTMP/a2/.hg/strip-backup/5fddd98957c8-f9244fa1-backup.hg (glob)

  $ hg tglog
  o  7: 'D'
  |
  o  6: 'C'
  |
  @  5: 'H'
  |
  | o  4: 'G'
  |/|
  o |  3: 'F'
  | |
  | o  2: 'E'
  |/
  | o  1: 'B'
  |/
  o  0: 'A'
  
  $ hg manifest --rev tip
  A
  C
  D
  F
  H

  $ cd ..


Rebasing B onto H using detach (same as not using it):

  $ hg clone -q -u . a a3
  $ cd a3

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
  
  $ hg rebase -s 1 -d 7
  rebasing 1:42ccdea3bb16 "B"
  rebasing 2:5fddd98957c8 "C"
  rebasing 3:32af7686d403 "D"
  saved backup bundle to $TESTTMP/a3/.hg/strip-backup/42ccdea3bb16-3cb021d3-backup.hg (glob)

  $ hg tglog
  o  7: 'D'
  |
  o  6: 'C'
  |
  o  5: 'B'
  |
  @  4: 'H'
  |
  | o  3: 'G'
  |/|
  o |  2: 'F'
  | |
  | o  1: 'E'
  |/
  o  0: 'A'
  
  $ hg manifest --rev tip
  A
  B
  C
  D
  F
  H

  $ cd ..


Rebasing C onto H detaching from B and collapsing:

  $ hg clone -q -u . a a4
  $ cd a4
  $ hg phase --force --secret 3

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
  
  $ hg rebase --collapse -s 2 -d 7
  rebasing 2:5fddd98957c8 "C"
  rebasing 3:32af7686d403 "D"
  saved backup bundle to $TESTTMP/a4/.hg/strip-backup/5fddd98957c8-f9244fa1-backup.hg (glob)

  $ hg  log -G --template "{rev}:{phase} '{desc}' {branches}\n"
  o  6:secret 'Collapsed revision
  |  * C
  |  * D'
  @  5:draft 'H'
  |
  | o  4:draft 'G'
  |/|
  o |  3:draft 'F'
  | |
  | o  2:draft 'E'
  |/
  | o  1:draft 'B'
  |/
  o  0:draft 'A'
  
  $ hg manifest --rev tip
  A
  C
  D
  F
  H

  $ cd ..

Rebasing across null as ancestor
  $ hg clone -q -U a a5

  $ cd a5

  $ echo x > x

  $ hg add x

  $ hg ci -m "extra branch"
  created new head

  $ hg tglog
  @  8: 'extra branch'
  
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
  
  $ hg rebase -s 1 -d tip
  rebasing 1:42ccdea3bb16 "B"
  rebasing 2:5fddd98957c8 "C"
  rebasing 3:32af7686d403 "D"
  saved backup bundle to $TESTTMP/a5/.hg/strip-backup/42ccdea3bb16-3cb021d3-backup.hg (glob)

  $ hg tglog
  o  8: 'D'
  |
  o  7: 'C'
  |
  o  6: 'B'
  |
  @  5: 'extra branch'
  
  o  4: 'H'
  |
  | o  3: 'G'
  |/|
  o |  2: 'F'
  | |
  | o  1: 'E'
  |/
  o  0: 'A'
  

  $ hg rebase -d 5 -s 7
  rebasing 7:13547172c9c0 "C"
  rebasing 8:4e27a76c371a "D" (tip)
  saved backup bundle to $TESTTMP/a5/.hg/strip-backup/13547172c9c0-35685ded-backup.hg (glob)
  $ hg tglog
  o  8: 'D'
  |
  o  7: 'C'
  |
  | o  6: 'B'
  |/
  @  5: 'extra branch'
  
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

Verify that target is not selected as external rev (issue3085)

  $ hg clone -q -U a a6
  $ cd a6
  $ hg up -q 6

  $ echo "I" >> E
  $ hg ci -m "I"
  $ hg merge 7
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m "Merge"
  $ echo "J" >> F
  $ hg ci -m "J"

  $ hg rebase -s 8 -d 7 --collapse --config ui.merge=internal:other
  rebasing 8:9790e768172d "I"
  rebasing 9:5d7b11f5fb97 "Merge"
  rebasing 10:9427d4d5af81 "J" (tip)
  saved backup bundle to $TESTTMP/a6/.hg/strip-backup/9790e768172d-c2111e9d-backup.hg (glob)

  $ hg tglog
  @  8: 'Collapsed revision
  |  * I
  |  * Merge
  |  * J'
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
  

  $ hg log --rev tip
  changeset:   8:9472f4b1d736
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Collapsed revision
  

  $ cd ..

Ensure --continue restores a correct state (issue3046) and phase:
  $ hg clone -q a a7
  $ cd a7
  $ hg up -q 3
  $ echo 'H2' > H
  $ hg ci -A -m 'H2'
  adding H
  $ hg phase --force --secret 8
  $ hg rebase -s 8 -d 7 --config ui.merge=internal:fail
  rebasing 8:6215fafa5447 "H2" (tip)
  merging H
  warning: conflicts while merging H! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ hg resolve --all -t internal:local
  (no more unresolved files)
  continue: hg rebase --continue
  $ hg rebase -c
  rebasing 8:6215fafa5447 "H2" (tip)
  note: rebase of 8:6215fafa5447 created no changes to commit
  saved backup bundle to $TESTTMP/a7/.hg/strip-backup/6215fafa5447-5804ebd5-backup.hg (glob)
  $ hg  log -G --template "{rev}:{phase} '{desc}' {branches}\n"
  @  7:draft 'H'
  |
  | o  6:draft 'G'
  |/|
  o |  5:draft 'F'
  | |
  | o  4:draft 'E'
  |/
  | o  3:draft 'D'
  | |
  | o  2:draft 'C'
  | |
  | o  1:draft 'B'
  |/
  o  0:draft 'A'
  

  $ cd ..
