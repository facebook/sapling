#chg-compatible

  $ enable rebase amend

  $ hg init a
  $ cd a
  $ hg unbundle "$TESTDIR/bundles/rebase.hg"
  adding changesets
  adding manifests
  adding file changes
  added 8 changesets with 7 changes to 7 files
  $ hg up tip
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ..


Rebasing
D onto H - simple rebase:
(this also tests that editor is invoked if '--edit' is specified, and that we
can abort or warn for colliding untracked files)

  $ hg clone -q -u . a a1
  $ cd a1

  $ tglog
  @  7: 02de42196ebe 'H'
  |
  | o  6: eea13746799a 'G'
  |/|
  o |  5: 24b6387c8c8c 'F'
  | |
  | o  4: 9520eea781bc 'E'
  |/
  | o  3: 32af7686d403 'D'
  | |
  | o  2: 5fddd98957c8 'C'
  | |
  | o  1: 42ccdea3bb16 'B'
  |/
  o  0: cd010b8cd998 'A'
  

  $ hg status --rev "desc(D)^1" --rev 'desc(D)'
  A D
  $ echo collide > D
  $ HGEDITOR=cat hg rebase -s 'desc(D)' -d 'desc(H)' --edit --config merge.checkunknown=warn
  rebasing 32af7686d403 "D"
  D: replacing untracked file
  D
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: Nicolas Dumazet <nicdumz.commits@gmail.com>
  HG: branch 'default'
  HG: added D
  $ cat D.orig
  collide
  $ rm D.orig

  $ tglog
  o  8: 1619f02ff7dd 'D'
  |
  @  7: 02de42196ebe 'H'
  |
  | o  6: eea13746799a 'G'
  |/|
  o |  5: 24b6387c8c8c 'F'
  | |
  | o  4: 9520eea781bc 'E'
  |/
  | o  2: 5fddd98957c8 'C'
  | |
  | o  1: 42ccdea3bb16 'B'
  |/
  o  0: cd010b8cd998 'A'
  
  $ cd ..


D onto F - intermediate point:
(this also tests that editor is not invoked if '--edit' is not specified, and
that we can ignore for colliding untracked files)

  $ hg clone -q -u . a a2
  $ cd a2
  $ echo collide > D

  $ HGEDITOR=cat hg rebase -s 'desc(D)' -d 'desc(F)' --config merge.checkunknown=ignore
  rebasing 32af7686d403 "D"
  $ cat D.orig
  collide
  $ rm D.orig

  $ tglog
  o  8: 2107530e74ab 'D'
  |
  | @  7: 02de42196ebe 'H'
  |/
  | o  6: eea13746799a 'G'
  |/|
  o |  5: 24b6387c8c8c 'F'
  | |
  | o  4: 9520eea781bc 'E'
  |/
  | o  2: 5fddd98957c8 'C'
  | |
  | o  1: 42ccdea3bb16 'B'
  |/
  o  0: cd010b8cd998 'A'
  
  $ cd ..


E onto H - skip of G:
(this also tests that we can overwrite untracked files and don't create backups
if they have the same contents)

  $ hg clone -q -u . a a3
  $ cd a3
  $ hg cat -r 'desc(E)' E | tee E
  E

  $ hg rebase -s 'desc(E)' -d 'desc(H)'
  rebasing 9520eea781bc "E"
  rebasing eea13746799a "G"
  note: rebase of 6:eea13746799a created no changes to commit
  $ f E.orig
  E.orig: file not found

  $ tglog
  o  8: 9f8b8ec77260 'E'
  |
  @  7: 02de42196ebe 'H'
  |
  o  5: 24b6387c8c8c 'F'
  |
  | o  3: 32af7686d403 'D'
  | |
  | o  2: 5fddd98957c8 'C'
  | |
  | o  1: 42ccdea3bb16 'B'
  |/
  o  0: cd010b8cd998 'A'
  
  $ cd ..


F onto E - rebase of a branching point (skip G):

  $ hg clone -q -u . a a4
  $ cd a4

  $ hg rebase -s 'desc(F)' -d 'desc(E)'
  rebasing 24b6387c8c8c "F"
  rebasing eea13746799a "G"
  note: rebase of 6:eea13746799a created no changes to commit
  rebasing 02de42196ebe "H"

  $ tglog
  @  9: e9240aeaa6ad 'H'
  |
  o  8: 5d0ccadb6e3e 'F'
  |
  o  4: 9520eea781bc 'E'
  |
  | o  3: 32af7686d403 'D'
  | |
  | o  2: 5fddd98957c8 'C'
  | |
  | o  1: 42ccdea3bb16 'B'
  |/
  o  0: cd010b8cd998 'A'
  
  $ cd ..


G onto H - merged revision having a parent in ancestors of target:

  $ hg clone -q -u . a a5
  $ cd a5

  $ hg rebase -s 'desc(G)' -d 'desc(H)'
  rebasing eea13746799a "G"

  $ tglog
  o    8: 397834907a90 'G'
  |\
  | @  7: 02de42196ebe 'H'
  | |
  | o  5: 24b6387c8c8c 'F'
  | |
  o |  4: 9520eea781bc 'E'
  |/
  | o  3: 32af7686d403 'D'
  | |
  | o  2: 5fddd98957c8 'C'
  | |
  | o  1: 42ccdea3bb16 'B'
  |/
  o  0: cd010b8cd998 'A'
  
  $ cd ..


F onto B - G maintains E as parent:

  $ hg clone -q -u . a a6
  $ cd a6

  $ hg rebase -s 'desc(F)' -d 'desc(B)'
  rebasing 24b6387c8c8c "F"
  rebasing eea13746799a "G"
  rebasing 02de42196ebe "H"

  $ tglog
  @  10: c87be72f9641 'H'
  |
  | o  9: 17badd73d4f1 'G'
  |/|
  o |  8: 74fb9ed646c4 'F'
  | |
  | o  4: 9520eea781bc 'E'
  | |
  | | o  3: 32af7686d403 'D'
  | | |
  +---o  2: 5fddd98957c8 'C'
  | |
  o |  1: 42ccdea3bb16 'B'
  |/
  o  0: cd010b8cd998 'A'
  
  $ cd ..


These will fail (using --source):

G onto F - rebase onto an ancestor:

  $ hg clone -q -u . a a7
  $ cd a7

  $ hg rebase -s 'desc(G)' -d 'desc(F)'
  nothing to rebase

F onto G - rebase onto a descendant:

  $ hg rebase -s 'desc(F)' -d 'desc(G)'
  abort: source and destination form a cycle
  [255]

G onto B - merge revision with both parents not in ancestors of target:

  $ hg rebase -s 'desc(G)' -d 'desc(B)'
  rebasing eea13746799a "G"
  abort: cannot rebase 6:eea13746799a without moving at least one of its parents
  [255]
  $ hg rebase --abort
  rebase aborted

These will abort gracefully (using --base):

G onto G - rebase onto same changeset:

  $ hg rebase -b 'desc(G)' -d 'desc(G)'
  nothing to rebase - eea13746799a is both "base" and destination

G onto F - rebase onto an ancestor:

  $ hg rebase -b 'desc(G)' -d 'desc(F)'
  nothing to rebase

F onto G - rebase onto a descendant:

  $ hg rebase -b 'desc(F)' -d 'desc(G)'
  nothing to rebase - "base" 24b6387c8c8c is already an ancestor of destination eea13746799a

C onto A - rebase onto an ancestor:

  $ hg rebase -d 'desc(A)' -s 'desc(C)'
  rebasing 5fddd98957c8 "C"
  rebasing 32af7686d403 "D"
  $ tglog
  o  9: c9659aac0000 'D'
  |
  o  8: e1c4361dd923 'C'
  |
  | @  7: 02de42196ebe 'H'
  | |
  | | o  6: eea13746799a 'G'
  | |/|
  | o |  5: 24b6387c8c8c 'F'
  |/ /
  | o  4: 9520eea781bc 'E'
  |/
  | o  1: 42ccdea3bb16 'B'
  |/
  o  0: cd010b8cd998 'A'
  

Check rebasing public changeset

  $ hg pull --config phases.publish=True -q -r 6 . # update phase of 6
  $ hg rebase -d 'desc(A)' -b 'desc(C)'
  nothing to rebase
  $ hg debugmakepublic e1c4361dd923
  $ hg rebase -d 'desc(H)' -b 'desc(C)'
  abort: can't rebase public changeset e1c4361dd923
  (see 'hg help phases' for details)
  [255]
  $ hg rebase -d 'desc(H)' -r 'desc(B) + (desc(C)::)'
  abort: can't rebase public changeset e1c4361dd923
  (see 'hg help phases' for details)
  [255]

  $ hg rebase -d 'desc(H)' -b 'desc(C)' --keep
  rebasing 42ccdea3bb16 "B"
  rebasing e1c4361dd923 "C" (public/e1c4361dd923d224beba950dfa5e53c861201386)
  rebasing c9659aac0000 "D"

Check rebasing mutable changeset
Source phase greater or equal to destination phase: new changeset get the phase of source:
  $ hg rebase -s'max(desc(D))' -d'desc(A)'
  rebasing 2b23e52411f4 "D"
  $ hg log --template "{phase}\n" -r 'max(desc(D))'
  draft
  $ hg rebase -s'max(desc(D))' -d'desc(B)'
  rebasing 2cb10d0cfc6c "D"
  $ hg log --template "{phase}\n" -r 'max(desc(D))'
  draft
  $ hg rebase -s'max(desc(D))' -d'desc(A)'
  rebasing 3fc1b42ad852 "D"
  $ hg log --template "{phase}\n" -r 'max(desc(D))'
  draft
  $ hg rebase -s'max(desc(D))' -d'desc(B)'
  rebasing 3838f70ff033 "D"
  $ hg log --template "{phase}\n" -r 'max(desc(D))'
  draft
Source phase lower than destination phase: new changeset get the phase of destination:
  $ hg rebase -s'max(desc(C))' -d'max(desc(D))'
  rebasing 6d4f22462821 "C"
  $ hg log --template "{phase}\n" -r 'rev(9)'
  draft

  $ cd ..

Check that temporary bundle doesn't lose phase when not using generaldelta

  $ hg --config format.usegeneraldelta=no init issue5678
  $ cd issue5678
  $ grep generaldelta .hg/requires
  [1]
  $ echo a > a
  $ hg ci -Aqm a
  $ echo b > b
  $ hg ci -Aqm b
  $ hg co -q '.^'
  $ echo c > c
  $ hg ci -Aqm c
  $ hg debugmakepublic .
  $ hg log -G -T '{rev}:{node|shortest} {phase} {desc}\n'
  @  2:d36c public c
  |
  | o  1:d2ae draft b
  |/
  o  0:cb9a public a
  
  $ hg rebase -s 'desc(b)' -d 'desc(c)'
  rebasing d2ae7f538514 "b"
  $ hg log -G -T '{rev}:{node|shortest} {phase} {desc}\n'
  o  3:c882 draft b
  |
  @  2:d36c public c
  |
  o  0:cb9a public a
  
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
  added 9 changesets with 9 changes to 9 files
  $ tglog
  o  8: 479ddb54a924 'I'
  |
  o  7: 72434a4e60b0 'H'
  |
  o  6: 3d8a618087a7 'G'
  |
  | o  5: 41bfcc75ed73 'F'
  | |
  | o  4: c01897464e7f 'E'
  |/
  o  3: ffd453c31098 'D'
  |
  o  2: c9e50f6cdc55 'C'
  |
  | o  1: 8fd0f7e49f53 'B'
  |/
  o  0: 9ae2ed22e576 'A'
  
  $ cd ..


Simple case with keep:

Source on have two descendant heads but ask for one

  $ hg clone -q -u . ah ah1
  $ cd ah1
  $ hg rebase -r 'max(desc(C))::desc(I)' -d 'desc(B)' -k
  rebasing c9e50f6cdc55 "C"
  rebasing ffd453c31098 "D"
  rebasing 3d8a618087a7 "G"
  rebasing 72434a4e60b0 "H"
  rebasing 479ddb54a924 "I"
  $ tglog
  o  13: 9bf1d9358a90 'I'
  |
  o  12: 274623a778d4 'H'
  |
  o  11: ab8c8617c8e8 'G'
  |
  o  10: c8cbf59f70da 'D'
  |
  o  9: 563e4faab485 'C'
  |
  | o  8: 479ddb54a924 'I'
  | |
  | o  7: 72434a4e60b0 'H'
  | |
  | o  6: 3d8a618087a7 'G'
  | |
  | | o  5: 41bfcc75ed73 'F'
  | | |
  | | o  4: c01897464e7f 'E'
  | |/
  | o  3: ffd453c31098 'D'
  | |
  | o  2: c9e50f6cdc55 'C'
  | |
  o |  1: 8fd0f7e49f53 'B'
  |/
  o  0: 9ae2ed22e576 'A'
  

  $ cd ..

Base on have one descendant heads we ask for but common ancestor have two

  $ hg clone -q -u . ah ah2
  $ cd ah2
  $ hg rebase -r 'desc(D)::desc(I)' -d 'desc(B)' --keep
  rebasing ffd453c31098 "D"
  rebasing 3d8a618087a7 "G"
  rebasing 72434a4e60b0 "H"
  rebasing 479ddb54a924 "I"
  $ tglog
  o  12: 9d7da0053b1c 'I'
  |
  o  11: 8fbd00952cbc 'H'
  |
  o  10: 51d434a615ee 'G'
  |
  o  9: a9c125634b0b 'D'
  |
  | o  8: 479ddb54a924 'I'
  | |
  | o  7: 72434a4e60b0 'H'
  | |
  | o  6: 3d8a618087a7 'G'
  | |
  | | o  5: 41bfcc75ed73 'F'
  | | |
  | | o  4: c01897464e7f 'E'
  | |/
  | o  3: ffd453c31098 'D'
  | |
  | o  2: c9e50f6cdc55 'C'
  | |
  o |  1: 8fd0f7e49f53 'B'
  |/
  o  0: 9ae2ed22e576 'A'
  

  $ cd ..

rebase subset

  $ hg clone -q -u . ah ah3
  $ cd ah3
  $ hg rebase -r 'desc(D)::desc(H)' -d 'desc(B)' --keep
  rebasing ffd453c31098 "D"
  rebasing 3d8a618087a7 "G"
  rebasing 72434a4e60b0 "H"
  $ tglog
  o  11: 8fbd00952cbc 'H'
  |
  o  10: 51d434a615ee 'G'
  |
  o  9: a9c125634b0b 'D'
  |
  | o  8: 479ddb54a924 'I'
  | |
  | o  7: 72434a4e60b0 'H'
  | |
  | o  6: 3d8a618087a7 'G'
  | |
  | | o  5: 41bfcc75ed73 'F'
  | | |
  | | o  4: c01897464e7f 'E'
  | |/
  | o  3: ffd453c31098 'D'
  | |
  | o  2: c9e50f6cdc55 'C'
  | |
  o |  1: 8fd0f7e49f53 'B'
  |/
  o  0: 9ae2ed22e576 'A'
  

  $ cd ..

rebase subset with multiple head

  $ hg clone -q -u . ah ah4
  $ cd ah4
  $ hg rebase -r 'desc(D)::(7+desc(F))' -d 'desc(B)' --keep
  rebasing ffd453c31098 "D"
  rebasing c01897464e7f "E"
  rebasing 41bfcc75ed73 "F"
  rebasing 3d8a618087a7 "G"
  rebasing 72434a4e60b0 "H"
  $ tglog
  o  13: 8fbd00952cbc 'H'
  |
  o  12: 51d434a615ee 'G'
  |
  | o  11: df23d8bda0b7 'F'
  | |
  | o  10: 47b7889448ff 'E'
  |/
  o  9: a9c125634b0b 'D'
  |
  | o  8: 479ddb54a924 'I'
  | |
  | o  7: 72434a4e60b0 'H'
  | |
  | o  6: 3d8a618087a7 'G'
  | |
  | | o  5: 41bfcc75ed73 'F'
  | | |
  | | o  4: c01897464e7f 'E'
  | |/
  | o  3: ffd453c31098 'D'
  | |
  | o  2: c9e50f6cdc55 'C'
  | |
  o |  1: 8fd0f7e49f53 'B'
  |/
  o  0: 9ae2ed22e576 'A'
  

  $ cd ..

More advanced tests

rebase on ancestor with revset

  $ hg clone -q -u . ah ah5
  $ cd ah5
  $ hg rebase -r 'desc(G)::' -d 'desc(C)'
  rebasing 3d8a618087a7 "G"
  rebasing 72434a4e60b0 "H"
  rebasing 479ddb54a924 "I"
  $ tglog
  o  11: fcb52e68a694 'I'
  |
  o  10: 77bd65cd7600 'H'
  |
  o  9: 12d0e738fb18 'G'
  |
  | o  5: 41bfcc75ed73 'F'
  | |
  | o  4: c01897464e7f 'E'
  | |
  | o  3: ffd453c31098 'D'
  |/
  o  2: c9e50f6cdc55 'C'
  |
  | o  1: 8fd0f7e49f53 'B'
  |/
  o  0: 9ae2ed22e576 'A'
  
  $ cd ..


rebase with multiple root.
We rebase E and G on B
We would expect heads are I, F if it was supported

  $ hg clone -q -u . ah ah6
  $ cd ah6
  $ hg rebase -r '(c01897464e7f3bb6f77cc94debcd48514133da09+3d8a618087a7b67fa87ecd461dcb049f5612ba77)::' -d 'desc(B)'
  rebasing c01897464e7f "E"
  rebasing 41bfcc75ed73 "F"
  rebasing 3d8a618087a7 "G"
  rebasing 72434a4e60b0 "H"
  rebasing 479ddb54a924 "I"
  $ tglog
  o  13: 9136df9a87cf 'I'
  |
  o  12: 23e8f30da832 'H'
  |
  o  11: b0efe8534e8b 'G'
  |
  | o  10: 6eb5b496ab79 'F'
  | |
  | o  9: d15eade9b0b1 'E'
  |/
  | o  3: ffd453c31098 'D'
  | |
  | o  2: c9e50f6cdc55 'C'
  | |
  o |  1: 8fd0f7e49f53 'B'
  |/
  o  0: 9ae2ed22e576 'A'
  
  $ cd ..

More complex rebase with multiple roots
each root have a different common ancestor with the destination and this is a detach

(setup)

  $ hg clone -q -u . a a8
  $ cd a8
  $ echo I > I
  $ hg add I
  $ hg commit -m I
  $ hg up 'desc(E)'
  1 files updated, 0 files merged, 3 files removed, 0 files unresolved
  $ echo I > J
  $ hg add J
  $ hg commit -m J
  $ echo I > K
  $ hg add K
  $ hg commit -m K
  $ tglog
  @  10: 23a4ace37988 'K'
  |
  o  9: 1301922eeb0c 'J'
  |
  | o  8: e7ec4e813ba6 'I'
  | |
  | o  7: 02de42196ebe 'H'
  | |
  +---o  6: eea13746799a 'G'
  | |/
  | o  5: 24b6387c8c8c 'F'
  | |
  o |  4: 9520eea781bc 'E'
  |/
  | o  3: 32af7686d403 'D'
  | |
  | o  2: 5fddd98957c8 'C'
  | |
  | o  1: 42ccdea3bb16 'B'
  |/
  o  0: cd010b8cd998 'A'
  
(actual test)

  $ hg rebase --dest 'desc(G)' --rev 'desc(K) + desc(I)'
  rebasing e7ec4e813ba6 "I"
  rebasing 23a4ace37988 "K"
  $ hg log --rev 'children(desc(G))'
  commit:      adb617877056
  parent:      eea13746799a
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     I
  
  commit:      882431a34a0e
  parent:      eea13746799a
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     K
  
  $ tglog
  @  12: 882431a34a0e 'K'
  |
  | o  11: adb617877056 'I'
  |/
  | o  9: 1301922eeb0c 'J'
  | |
  | | o  7: 02de42196ebe 'H'
  | | |
  o---+  6: eea13746799a 'G'
  |/ /
  | o  5: 24b6387c8c8c 'F'
  | |
  o |  4: 9520eea781bc 'E'
  |/
  | o  3: 32af7686d403 'D'
  | |
  | o  2: 5fddd98957c8 'C'
  | |
  | o  1: 42ccdea3bb16 'B'
  |/
  o  0: cd010b8cd998 'A'
  

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
  $ hg up 'desc(initial)'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ touch other-file
  $ hg add other-file
  $ hg commit -m 'first source commit'
  $ mkdir subdir
  $ cd subdir
  $ touch subfile
  $ hg add subfile
  $ hg commit -m 'second source with subdir'

  $ hg rebase -b . -d 'desc(dest)' --traceback
  rebasing 779a07b1b7a0 "first source commit"
  current directory was removed (rmcwd !)
  (consider changing to repo root: $TESTTMP/cwd-vanish) (rmcwd !)
  rebasing a7d6f3a00bf3 "second source with subdir"

Get back to the root of cwd-vanish. Note that even though `cd ..`
works on most systems, it does not work on FreeBSD 10, so we use an
absolute path to get back to the repository.
  $ cd $TESTTMP

Test that rebase is done in topo order (issue5370)

  $ hg init order
  $ cd order
  $ touch a && hg add a && hg ci -m A
  $ touch b && hg add b && hg ci -m B
  $ touch c && hg add c && hg ci -m C
  $ hg up 'desc(B)'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ touch d && hg add d && hg ci -m D
  $ hg up 'desc(C)'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ touch e && hg add e && hg ci -m E
  $ hg up 'desc(D)'
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ touch f && hg add f && hg ci -m F
  $ hg up 'desc(A)'
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved
  $ touch g && hg add g && hg ci -m G

  $ tglog
  @  6: 124bb27b6f28 'G'
  |
  | o  5: 412b391de760 'F'
  | |
  | | o  4: 82ae8dc7a9b7 'E'
  | | |
  | o |  3: ab709c9f7171 'D'
  | | |
  | | o  2: d84f5cfaaf14 'C'
  | |/
  | o  1: 76035bbd54bd 'B'
  |/
  o  0: 216878401574 'A'
  

  $ hg rebase -s 'desc(B)' -d 'desc(G)'
  rebasing 76035bbd54bd "B"
  rebasing d84f5cfaaf14 "C"
  rebasing 82ae8dc7a9b7 "E"
  rebasing ab709c9f7171 "D"
  rebasing 412b391de760 "F"

  $ tglog
  o  11: 31884cfb735e 'F'
  |
  o  10: 6d89fa5b0909 'D'
  |
  | o  9: de64d97c697b 'E'
  | |
  | o  8: b18e4d2d0aa1 'C'
  |/
  o  7: 0983daf9ff6a 'B'
  |
  @  6: 124bb27b6f28 'G'
  |
  o  0: 216878401574 'A'
  

Test experimental revset
========================

  $ cd ../cwd-vanish

Make the repo a bit more interesting

  $ hg up 'desc(dest)'
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ echo aaa > aaa
  $ hg add aaa
  $ hg commit -m aaa
  $ hg log -G
  @  commit:      5f7bc9025ed2
  |  parent:      58d79cc1cf43
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     aaa
  |
  | o  commit:      1910d5ff34ea
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     second source with subdir
  | |
  | o  commit:      82901330b6ef
  |/   parent:      58d79cc1cf43
  |    user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     first source commit
  |
  o  commit:      58d79cc1cf43
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     dest commit
  |
  o  commit:      e94b687f7da3
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     initial commit
  

Testing from lower head

  $ hg up 'desc(second)'
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg log -r '_destrebase()'
  commit:      5f7bc9025ed2
  parent:      58d79cc1cf43
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     aaa
  

Testing from upper head

  $ hg log -r '_destrebase(desc(aaa))'
  commit:      1910d5ff34ea
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     second source with subdir
  
  $ hg up 'desc(aaa)'
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg log -r '_destrebase()'
  commit:      1910d5ff34ea
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     second source with subdir
  
Testing rebase being called inside another transaction

  $ cd $TESTTMP
  $ hg init tr-state
  $ cd tr-state
  $ cat > $TESTTMP/wraprebase.py <<EOF
  > from __future__ import absolute_import
  > from edenscm.mercurial import extensions
  > def _rebase(orig, ui, repo, *args, **kwargs):
  >     with repo.wlock():
  >         with repo.lock():
  >             with repo.transaction('wrappedrebase'):
  >                 return orig(ui, repo, *args, **kwargs)
  > def wraprebase(loaded):
  >     assert loaded
  >     rebasemod = extensions.find('rebase')
  >     extensions.wrapcommand(rebasemod.cmdtable, 'rebase', _rebase)
  > def extsetup(ui):
  >     extensions.afterloaded('rebase', wraprebase)
  > EOF

  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > wraprebase=$TESTTMP/wraprebase.py
  > [experimental]
  > evolution=true
  > EOF

  $ hg debugdrawdag <<'EOS'
  > B C
  > |/
  > A
  > EOS

  $ hg rebase -s C -d B
  rebasing dc0947a82db8 "C" (C)

  $ [ -f .hg/rebasestate ] && echo 'WRONG: rebasestate should not exist'
  [1]
