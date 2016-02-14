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


Rebasing
D onto H - simple rebase:
(this also tests that editor is invoked if '--edit' is specified, and that we
can abort or warn for colliding untracked files)

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
  

  $ hg status --rev "3^1" --rev 3
  A D
  $ echo collide > D
  $ HGEDITOR=cat hg rebase -s 3 -d 7 --edit --config merge.checkunknown=warn
  rebasing 3:32af7686d403 "D"
  D: replacing untracked file
  D
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: Nicolas Dumazet <nicdumz.commits@gmail.com>
  HG: branch 'default'
  HG: added D
  saved backup bundle to $TESTTMP/a1/.hg/strip-backup/32af7686d403-6f7dface-backup.hg (glob)
  $ cat D.orig
  collide
  $ rm D.orig

  $ hg tglog
  o  7: 'D'
  |
  @  6: 'H'
  |
  | o  5: 'G'
  |/|
  o |  4: 'F'
  | |
  | o  3: 'E'
  |/
  | o  2: 'C'
  | |
  | o  1: 'B'
  |/
  o  0: 'A'
  
  $ cd ..


D onto F - intermediate point:
(this also tests that editor is not invoked if '--edit' is not specified, and
that we can ignore for colliding untracked files)

  $ hg clone -q -u . a a2
  $ cd a2
  $ echo collide > D

  $ HGEDITOR=cat hg rebase -s 3 -d 5 --config merge.checkunknown=ignore
  rebasing 3:32af7686d403 "D"
  saved backup bundle to $TESTTMP/a2/.hg/strip-backup/32af7686d403-6f7dface-backup.hg (glob)
  $ cat D.orig
  collide
  $ rm D.orig

  $ hg tglog
  o  7: 'D'
  |
  | @  6: 'H'
  |/
  | o  5: 'G'
  |/|
  o |  4: 'F'
  | |
  | o  3: 'E'
  |/
  | o  2: 'C'
  | |
  | o  1: 'B'
  |/
  o  0: 'A'
  
  $ cd ..


E onto H - skip of G:
(this also tests that we can overwrite untracked files and don't create backups
if they have the same contents)

  $ hg clone -q -u . a a3
  $ cd a3
  $ hg cat -r 4 E | tee E
  E

  $ hg rebase -s 4 -d 7
  rebasing 4:9520eea781bc "E"
  rebasing 6:eea13746799a "G"
  note: rebase of 6:eea13746799a created no changes to commit
  saved backup bundle to $TESTTMP/a3/.hg/strip-backup/9520eea781bc-fcd8edd4-backup.hg (glob)
  $ f E.orig
  E.orig: file not found

  $ hg tglog
  o  6: 'E'
  |
  @  5: 'H'
  |
  o  4: 'F'
  |
  | o  3: 'D'
  | |
  | o  2: 'C'
  | |
  | o  1: 'B'
  |/
  o  0: 'A'
  
  $ cd ..


F onto E - rebase of a branching point (skip G):

  $ hg clone -q -u . a a4
  $ cd a4

  $ hg rebase -s 5 -d 4
  rebasing 5:24b6387c8c8c "F"
  rebasing 6:eea13746799a "G"
  note: rebase of 6:eea13746799a created no changes to commit
  rebasing 7:02de42196ebe "H" (tip)
  saved backup bundle to $TESTTMP/a4/.hg/strip-backup/24b6387c8c8c-c3fe765d-backup.hg (glob)

  $ hg tglog
  @  6: 'H'
  |
  o  5: 'F'
  |
  o  4: 'E'
  |
  | o  3: 'D'
  | |
  | o  2: 'C'
  | |
  | o  1: 'B'
  |/
  o  0: 'A'
  
  $ cd ..


G onto H - merged revision having a parent in ancestors of target:

  $ hg clone -q -u . a a5
  $ cd a5

  $ hg rebase -s 6 -d 7
  rebasing 6:eea13746799a "G"
  saved backup bundle to $TESTTMP/a5/.hg/strip-backup/eea13746799a-883828ed-backup.hg (glob)

  $ hg tglog
  o    7: 'G'
  |\
  | @  6: 'H'
  | |
  | o  5: 'F'
  | |
  o |  4: 'E'
  |/
  | o  3: 'D'
  | |
  | o  2: 'C'
  | |
  | o  1: 'B'
  |/
  o  0: 'A'
  
  $ cd ..


F onto B - G maintains E as parent:

  $ hg clone -q -u . a a6
  $ cd a6

  $ hg rebase -s 5 -d 1
  rebasing 5:24b6387c8c8c "F"
  rebasing 6:eea13746799a "G"
  rebasing 7:02de42196ebe "H" (tip)
  saved backup bundle to $TESTTMP/a6/.hg/strip-backup/24b6387c8c8c-c3fe765d-backup.hg (glob)

  $ hg tglog
  @  7: 'H'
  |
  | o  6: 'G'
  |/|
  o |  5: 'F'
  | |
  | o  4: 'E'
  | |
  | | o  3: 'D'
  | | |
  +---o  2: 'C'
  | |
  o |  1: 'B'
  |/
  o  0: 'A'
  
  $ cd ..


These will fail (using --source):

G onto F - rebase onto an ancestor:

  $ hg clone -q -u . a a7
  $ cd a7

  $ hg rebase -s 6 -d 5
  nothing to rebase
  [1]

F onto G - rebase onto a descendant:

  $ hg rebase -s 5 -d 6
  abort: source is ancestor of destination
  [255]

G onto B - merge revision with both parents not in ancestors of target:

  $ hg rebase -s 6 -d 1
  rebasing 6:eea13746799a "G"
  abort: cannot use revision 6 as base, result would have 3 parents
  [255]


These will abort gracefully (using --base):

G onto G - rebase onto same changeset:

  $ hg rebase -b 6 -d 6
  nothing to rebase - eea13746799a is both "base" and destination
  [1]

G onto F - rebase onto an ancestor:

  $ hg rebase -b 6 -d 5
  nothing to rebase
  [1]

F onto G - rebase onto a descendant:

  $ hg rebase -b 5 -d 6
  nothing to rebase - "base" 24b6387c8c8c is already an ancestor of destination eea13746799a
  [1]

C onto A - rebase onto an ancestor:

  $ hg rebase -d 0 -s 2
  rebasing 2:5fddd98957c8 "C"
  rebasing 3:32af7686d403 "D"
  saved backup bundle to $TESTTMP/a7/.hg/strip-backup/5fddd98957c8-f9244fa1-backup.hg (glob)
  $ hg tglog
  o  7: 'D'
  |
  o  6: 'C'
  |
  | @  5: 'H'
  | |
  | | o  4: 'G'
  | |/|
  | o |  3: 'F'
  |/ /
  | o  2: 'E'
  |/
  | o  1: 'B'
  |/
  o  0: 'A'
  

Check rebasing public changeset

  $ hg pull --config phases.publish=True -q -r 6 . # update phase of 6
  $ hg rebase -d 0 -b 6
  nothing to rebase
  [1]
  $ hg rebase -d 5 -b 6
  abort: can't rebase public changeset e1c4361dd923
  (see "hg help phases" for details)
  [255]

  $ hg rebase -d 5 -b 6 --keep
  rebasing 6:e1c4361dd923 "C"
  rebasing 7:c9659aac0000 "D" (tip)

Check rebasing mutable changeset
Source phase greater or equal to destination phase: new changeset get the phase of source:
  $ hg id -n
  5
  $ hg rebase -s9 -d0
  rebasing 9:2b23e52411f4 "D" (tip)
  saved backup bundle to $TESTTMP/a7/.hg/strip-backup/2b23e52411f4-f942decf-backup.hg (glob)
  $ hg id -n # check we updated back to parent
  5
  $ hg log --template "{phase}\n" -r 9
  draft
  $ hg rebase -s9 -d1
  rebasing 9:2cb10d0cfc6c "D" (tip)
  saved backup bundle to $TESTTMP/a7/.hg/strip-backup/2cb10d0cfc6c-ddb0f256-backup.hg (glob)
  $ hg log --template "{phase}\n" -r 9
  draft
  $ hg phase --force --secret 9
  $ hg rebase -s9 -d0
  rebasing 9:c5b12b67163a "D" (tip)
  saved backup bundle to $TESTTMP/a7/.hg/strip-backup/c5b12b67163a-4e372053-backup.hg (glob)
  $ hg log --template "{phase}\n" -r 9
  secret
  $ hg rebase -s9 -d1
  rebasing 9:2a0524f868ac "D" (tip)
  saved backup bundle to $TESTTMP/a7/.hg/strip-backup/2a0524f868ac-cefd8574-backup.hg (glob)
  $ hg log --template "{phase}\n" -r 9
  secret
Source phase lower than destination phase: new changeset get the phase of destination:
  $ hg rebase -s8 -d9
  rebasing 8:6d4f22462821 "C"
  saved backup bundle to $TESTTMP/a7/.hg/strip-backup/6d4f22462821-3441f70b-backup.hg (glob)
  $ hg log --template "{phase}\n" -r 'rev(9)'
  secret

  $ cd ..

Test for revset

We need a bit different graph
All destination are B

  $ hg init ah
  $ cd ah
  $ hg unbundle "$TESTDIR/bundles/rebase-revset.hg"
  adding changesets
  adding manifests
  adding file changes
  added 9 changesets with 9 changes to 9 files (+2 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hg tglog
  o  8: 'I'
  |
  o  7: 'H'
  |
  o  6: 'G'
  |
  | o  5: 'F'
  | |
  | o  4: 'E'
  |/
  o  3: 'D'
  |
  o  2: 'C'
  |
  | o  1: 'B'
  |/
  o  0: 'A'
  
  $ cd ..


Simple case with keep:

Source on have two descendant heads but ask for one

  $ hg clone -q -u . ah ah1
  $ cd ah1
  $ hg rebase -r '2::8' -d 1
  abort: can't remove original changesets with unrebased descendants
  (use --keep to keep original changesets)
  [255]
  $ hg rebase -r '2::8' -d 1 -k
  rebasing 2:c9e50f6cdc55 "C"
  rebasing 3:ffd453c31098 "D"
  rebasing 6:3d8a618087a7 "G"
  rebasing 7:72434a4e60b0 "H"
  rebasing 8:479ddb54a924 "I" (tip)
  $ hg tglog
  o  13: 'I'
  |
  o  12: 'H'
  |
  o  11: 'G'
  |
  o  10: 'D'
  |
  o  9: 'C'
  |
  | o  8: 'I'
  | |
  | o  7: 'H'
  | |
  | o  6: 'G'
  | |
  | | o  5: 'F'
  | | |
  | | o  4: 'E'
  | |/
  | o  3: 'D'
  | |
  | o  2: 'C'
  | |
  o |  1: 'B'
  |/
  o  0: 'A'
  

  $ cd ..

Base on have one descendant heads we ask for but common ancestor have two

  $ hg clone -q -u . ah ah2
  $ cd ah2
  $ hg rebase -r '3::8' -d 1
  abort: can't remove original changesets with unrebased descendants
  (use --keep to keep original changesets)
  [255]
  $ hg rebase -r '3::8' -d 1 --keep
  rebasing 3:ffd453c31098 "D"
  rebasing 6:3d8a618087a7 "G"
  rebasing 7:72434a4e60b0 "H"
  rebasing 8:479ddb54a924 "I" (tip)
  $ hg tglog
  o  12: 'I'
  |
  o  11: 'H'
  |
  o  10: 'G'
  |
  o  9: 'D'
  |
  | o  8: 'I'
  | |
  | o  7: 'H'
  | |
  | o  6: 'G'
  | |
  | | o  5: 'F'
  | | |
  | | o  4: 'E'
  | |/
  | o  3: 'D'
  | |
  | o  2: 'C'
  | |
  o |  1: 'B'
  |/
  o  0: 'A'
  

  $ cd ..

rebase subset

  $ hg clone -q -u . ah ah3
  $ cd ah3
  $ hg rebase -r '3::7' -d 1
  abort: can't remove original changesets with unrebased descendants
  (use --keep to keep original changesets)
  [255]
  $ hg rebase -r '3::7' -d 1 --keep
  rebasing 3:ffd453c31098 "D"
  rebasing 6:3d8a618087a7 "G"
  rebasing 7:72434a4e60b0 "H"
  $ hg tglog
  o  11: 'H'
  |
  o  10: 'G'
  |
  o  9: 'D'
  |
  | o  8: 'I'
  | |
  | o  7: 'H'
  | |
  | o  6: 'G'
  | |
  | | o  5: 'F'
  | | |
  | | o  4: 'E'
  | |/
  | o  3: 'D'
  | |
  | o  2: 'C'
  | |
  o |  1: 'B'
  |/
  o  0: 'A'
  

  $ cd ..

rebase subset with multiple head

  $ hg clone -q -u . ah ah4
  $ cd ah4
  $ hg rebase -r '3::(7+5)' -d 1
  abort: can't remove original changesets with unrebased descendants
  (use --keep to keep original changesets)
  [255]
  $ hg rebase -r '3::(7+5)' -d 1 --keep
  rebasing 3:ffd453c31098 "D"
  rebasing 4:c01897464e7f "E"
  rebasing 5:41bfcc75ed73 "F"
  rebasing 6:3d8a618087a7 "G"
  rebasing 7:72434a4e60b0 "H"
  $ hg tglog
  o  13: 'H'
  |
  o  12: 'G'
  |
  | o  11: 'F'
  | |
  | o  10: 'E'
  |/
  o  9: 'D'
  |
  | o  8: 'I'
  | |
  | o  7: 'H'
  | |
  | o  6: 'G'
  | |
  | | o  5: 'F'
  | | |
  | | o  4: 'E'
  | |/
  | o  3: 'D'
  | |
  | o  2: 'C'
  | |
  o |  1: 'B'
  |/
  o  0: 'A'
  

  $ cd ..

More advanced tests

rebase on ancestor with revset

  $ hg clone -q -u . ah ah5
  $ cd ah5
  $ hg rebase -r '6::' -d 2
  rebasing 6:3d8a618087a7 "G"
  rebasing 7:72434a4e60b0 "H"
  rebasing 8:479ddb54a924 "I" (tip)
  saved backup bundle to $TESTTMP/ah5/.hg/strip-backup/3d8a618087a7-b4f73f31-backup.hg (glob)
  $ hg tglog
  o  8: 'I'
  |
  o  7: 'H'
  |
  o  6: 'G'
  |
  | o  5: 'F'
  | |
  | o  4: 'E'
  | |
  | o  3: 'D'
  |/
  o  2: 'C'
  |
  | o  1: 'B'
  |/
  o  0: 'A'
  
  $ cd ..


rebase with multiple root.
We rebase E and G on B
We would expect heads are I, F if it was supported

  $ hg clone -q -u . ah ah6
  $ cd ah6
  $ hg rebase -r '(4+6)::' -d 1
  rebasing 4:c01897464e7f "E"
  rebasing 5:41bfcc75ed73 "F"
  rebasing 6:3d8a618087a7 "G"
  rebasing 7:72434a4e60b0 "H"
  rebasing 8:479ddb54a924 "I" (tip)
  saved backup bundle to $TESTTMP/ah6/.hg/strip-backup/3d8a618087a7-aae93a24-backup.hg (glob)
  $ hg tglog
  o  8: 'I'
  |
  o  7: 'H'
  |
  o  6: 'G'
  |
  | o  5: 'F'
  | |
  | o  4: 'E'
  |/
  | o  3: 'D'
  | |
  | o  2: 'C'
  | |
  o |  1: 'B'
  |/
  o  0: 'A'
  
  $ cd ..

More complex rebase with multiple roots
each root have a different common ancestor with the destination and this is a detach

(setup)

  $ hg clone -q -u . a a8
  $ cd a8
  $ echo I > I
  $ hg add I
  $ hg commit -m I
  $ hg up 4
  1 files updated, 0 files merged, 3 files removed, 0 files unresolved
  $ echo I > J
  $ hg add J
  $ hg commit -m J
  created new head
  $ echo I > K
  $ hg add K
  $ hg commit -m K
  $ hg tglog
  @  10: 'K'
  |
  o  9: 'J'
  |
  | o  8: 'I'
  | |
  | o  7: 'H'
  | |
  +---o  6: 'G'
  | |/
  | o  5: 'F'
  | |
  o |  4: 'E'
  |/
  | o  3: 'D'
  | |
  | o  2: 'C'
  | |
  | o  1: 'B'
  |/
  o  0: 'A'
  
(actual test)

  $ hg rebase --dest 'desc(G)' --rev 'desc(K) + desc(I)'
  rebasing 8:e7ec4e813ba6 "I"
  rebasing 10:23a4ace37988 "K" (tip)
  saved backup bundle to $TESTTMP/a8/.hg/strip-backup/23a4ace37988-b06984b3-backup.hg (glob)
  $ hg log --rev 'children(desc(G))'
  changeset:   9:adb617877056
  parent:      6:eea13746799a
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     I
  
  changeset:   10:882431a34a0e
  tag:         tip
  parent:      6:eea13746799a
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     K
  
  $ hg tglog
  @  10: 'K'
  |
  | o  9: 'I'
  |/
  | o  8: 'J'
  | |
  | | o  7: 'H'
  | | |
  o---+  6: 'G'
  |/ /
  | o  5: 'F'
  | |
  o |  4: 'E'
  |/
  | o  3: 'D'
  | |
  | o  2: 'C'
  | |
  | o  1: 'B'
  |/
  o  0: 'A'
  

Test that rebase is not confused by $CWD disappearing during rebase (issue4121)

  $ cd ..
  $ hg init cwd-vanish
  $ cd cwd-vanish
  $ touch initial-file
  $ hg add initial-file
  $ hg commit -m 'initial commit'
  $ touch dest-file
  $ hg add dest-file
  $ hg commit -m 'dest commit'
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ touch other-file
  $ hg add other-file
  $ hg commit -m 'first source commit'
  created new head
  $ mkdir subdir
  $ cd subdir
  $ touch subfile
  $ hg add subfile
  $ hg commit -m 'second source with subdir'
  $ hg rebase -b . -d 1 --traceback
  rebasing 2:779a07b1b7a0 "first source commit"
  rebasing 3:a7d6f3a00bf3 "second source with subdir" (tip)
  saved backup bundle to $TESTTMP/cwd-vanish/.hg/strip-backup/779a07b1b7a0-853e0073-backup.hg (glob)

Test experimental revset
========================

  $ cd ..

Make the repo a bit more interresting

  $ hg up 1
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ echo aaa > aaa
  $ hg add aaa
  $ hg commit -m aaa
  created new head
  $ hg log -G
  @  changeset:   4:5f7bc9025ed2
  |  tag:         tip
  |  parent:      1:58d79cc1cf43
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     aaa
  |
  | o  changeset:   3:1910d5ff34ea
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     second source with subdir
  | |
  | o  changeset:   2:82901330b6ef
  |/   user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     first source commit
  |
  o  changeset:   1:58d79cc1cf43
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     dest commit
  |
  o  changeset:   0:e94b687f7da3
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     initial commit
  

Testing from lower head

  $ hg up 3
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg log -r '_destrebase()'
  changeset:   4:5f7bc9025ed2
  tag:         tip
  parent:      1:58d79cc1cf43
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     aaa
  

Testing from upper head

  $ hg log -r '_destrebase(4)'
  changeset:   3:1910d5ff34ea
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     second source with subdir
  
  $ hg up 4
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg log -r '_destrebase()'
  changeset:   3:1910d5ff34ea
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     second source with subdir
  
