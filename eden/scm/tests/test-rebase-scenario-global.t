#chg-compatible
  $ setconfig status.use-rust=False workingcopy.use-rust=False
  $ setconfig workingcopy.ruststatus=False
  $ setconfig experimental.allowfilepeer=True
  $ setconfig format.use-segmented-changelog=true
  $ setconfig devel.segmented-changelog-rev-compat=true

  $ configure dummyssh
  $ enable rebase amend

  $ hg init a
  $ cd a
  $ setconfig extensions.treemanifest=$TESTDIR/../edenscm/ext/treemanifestserver.py
  $ setconfig treemanifest.server=True

  $ echo A > A
  $ hg commit -Aqm "A"
  $ echo B > B
  $ hg commit -Aqm "B"
  $ echo C > C
  $ hg commit -Aqm "C"
  $ echo D > D
  $ hg commit -Aqm "D"
  $ hg up -q .~3
  $ echo E > E
  $ hg commit -Aqm "E"
  $ hg book E
  $ hg up -q .~1
  $ echo F > F
  $ hg commit -Aqm "F"
  $ hg merge -q E
  $ hg book -d E
  $ echo G > G
  $ hg commit -Aqm "G"
  $ hg up -q .^
  $ echo H > H
  $ hg commit -Aqm "H"
  $ cd ..


Rebasing
D onto H - simple rebase:
(this also tests that editor is invoked if '--edit' is specified, and that we
can abort or warn for colliding untracked files)

  $ hg clone -q -u . ssh://user@dummy/a a1
  $ cd a1

  $ tglog
  @  15ed2d917603 'H'
  │
  │ o  3dbfcf9931fb 'G'
  ╭─┤
  o │  c137c2b8081f 'F'
  │ │
  │ o  4e18486b3568 'E'
  ├─╯
  │ o  b3325c91a4d9 'D'
  │ │
  │ o  f838bfaca5c7 'C'
  │ │
  │ o  27547f69f254 'B'
  ├─╯
  o  4a2df7238c3b 'A'
  

  $ hg status --rev "desc(D)^1" --rev 'desc(D)'
  A D
  $ echo collide > D
  $ HGEDITOR=cat hg rebase -s 'desc(D)' -d 'desc(H)' --edit --config merge.checkunknown=warn
  rebasing b3325c91a4d9 "D"
  D: replacing untracked file
  D
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: test
  HG: branch 'default'
  HG: added D

  $ tglog
  o  bdad251407f5 'D'
  │
  @  15ed2d917603 'H'
  │
  │ o  3dbfcf9931fb 'G'
  ╭─┤
  o │  c137c2b8081f 'F'
  │ │
  │ o  4e18486b3568 'E'
  ├─╯
  │ o  f838bfaca5c7 'C'
  │ │
  │ o  27547f69f254 'B'
  ├─╯
  o  4a2df7238c3b 'A'
  
  $ cd ..


D onto F - intermediate point:
(this also tests that editor is not invoked if '--edit' is not specified, and
that we can ignore for colliding untracked files)

  $ hg clone -q -u . ssh://user@dummy/a a2
  $ cd a2
  $ echo collide > D

  $ HGEDITOR=cat hg rebase -s 'desc(D)' -d 'desc(F)' --config merge.checkunknown=ignore
  rebasing b3325c91a4d9 "D"

  $ tglog
  o  80272bc566a5 'D'
  │
  │ @  15ed2d917603 'H'
  ├─╯
  │ o  3dbfcf9931fb 'G'
  ╭─┤
  o │  c137c2b8081f 'F'
  │ │
  │ o  4e18486b3568 'E'
  ├─╯
  │ o  f838bfaca5c7 'C'
  │ │
  │ o  27547f69f254 'B'
  ├─╯
  o  4a2df7238c3b 'A'
  
  $ cd ..


E onto H - skip of G:
(this also tests that we can overwrite untracked files and don't create backups
if they have the same contents)

  $ hg clone -q -u . ssh://user@dummy/a a3
  $ cd a3
  $ hg cat -r 'desc(E)' E | tee E
  E

  $ hg rebase -s 'desc(E)' -d 'desc(H)'
  rebasing 4e18486b3568 "E"
  rebasing 3dbfcf9931fb "G"
  $ f E.orig
  E.orig: file not found

  $ tglog
  o  2a7617a60286 'G'
  │
  o  2715de1086ea 'E'
  │
  @  15ed2d917603 'H'
  │
  o  c137c2b8081f 'F'
  │
  │ o  b3325c91a4d9 'D'
  │ │
  │ o  f838bfaca5c7 'C'
  │ │
  │ o  27547f69f254 'B'
  ├─╯
  o  4a2df7238c3b 'A'
  
  $ cd ..


F onto E - rebase of a branching point (skip G):

  $ hg clone -q -u . ssh://user@dummy/a a4
  $ cd a4

  $ hg rebase -s 'desc(F)' -d 'desc(E)'
  rebasing c137c2b8081f "F"
  rebasing 3dbfcf9931fb "G"
  rebasing 15ed2d917603 "H"

  $ tglog
  @  fb83ea250ea6 'H'
  │
  │ o  ffbb9c437931 'G'
  ├─╯
  o  8d8f63bc5025 'F'
  │
  o  4e18486b3568 'E'
  │
  │ o  b3325c91a4d9 'D'
  │ │
  │ o  f838bfaca5c7 'C'
  │ │
  │ o  27547f69f254 'B'
  ├─╯
  o  4a2df7238c3b 'A'
  
  $ cd ..


G onto H - merged revision having a parent in ancestors of target:

  $ hg clone -q -u . ssh://user@dummy/a a5
  $ cd a5

  $ hg rebase -s 'desc(G)' -d 'desc(H)'
  rebasing 3dbfcf9931fb "G"

  $ tglog
  o    8483d57ef6a6 'G'
  ├─╮
  │ @  15ed2d917603 'H'
  │ │
  │ o  c137c2b8081f 'F'
  │ │
  o │  4e18486b3568 'E'
  ├─╯
  │ o  b3325c91a4d9 'D'
  │ │
  │ o  f838bfaca5c7 'C'
  │ │
  │ o  27547f69f254 'B'
  ├─╯
  o  4a2df7238c3b 'A'
  
  $ cd ..


F onto B - G maintains E as parent:

  $ hg clone -q -u . ssh://user@dummy/a a6
  $ cd a6

  $ hg rebase -s 'desc(F)' -d 'desc(B)'
  rebasing c137c2b8081f "F"
  rebasing 3dbfcf9931fb "G"
  rebasing 15ed2d917603 "H"

  $ tglog
  @  a181cc259486 'H'
  │
  │ o  386251d51cd6 'G'
  ╭─┤
  o │  302673a1e012 'F'
  │ │
  │ o  4e18486b3568 'E'
  │ │
  │ │ o  b3325c91a4d9 'D'
  │ │ │
  │ │ o  f838bfaca5c7 'C'
  ├───╯
  o │  27547f69f254 'B'
  ├─╯
  o  4a2df7238c3b 'A'
  
  $ cd ..


These will fail (using --source):

G onto F - rebase onto an ancestor:

  $ hg clone -q -u . ssh://user@dummy/a a7
  $ cd a7

  $ hg rebase -s 'desc(G)' -d 'desc(F)'
  nothing to rebase

F onto G - rebase onto a descendant:

  $ hg rebase -s 'desc(F)' -d 'desc(G)'
  abort: source and destination form a cycle
  [255]

G onto B - merge revision with both parents not in ancestors of target:

  $ hg rebase -s 'desc(G)' -d 'desc(B)'
  rebasing 3dbfcf9931fb "G"
  abort: cannot rebase 3dbfcf9931fb without moving at least one of its parents
  [255]
  $ hg rebase --abort
  rebase aborted

These will abort gracefully (using --base):

G onto G - rebase onto same changeset:

  $ hg rebase -b 'desc(G)' -d 'desc(G)'
  nothing to rebase - 3dbfcf9931fb is both "base" and destination

G onto F - rebase onto an ancestor:

  $ hg rebase -b 'desc(G)' -d 'desc(F)'
  nothing to rebase

F onto G - rebase onto a descendant:

  $ hg rebase -b 'desc(F)' -d 'desc(G)'
  nothing to rebase - "base" c137c2b8081f is already an ancestor of destination 3dbfcf9931fb

C onto A - rebase onto an ancestor:

  $ hg rebase -d 'desc(A)' -s 'desc(C)'
  rebasing f838bfaca5c7 "C"
  rebasing b3325c91a4d9 "D"
  $ tglog
  o  3f51ceb7b044 'D'
  │
  o  7d3e5262cbd7 'C'
  │
  │ @  15ed2d917603 'H'
  │ │
  │ │ o  3dbfcf9931fb 'G'
  │ ╭─┤
  │ o │  c137c2b8081f 'F'
  ├─╯ │
  │   o  4e18486b3568 'E'
  ├───╯
  │ o  27547f69f254 'B'
  ├─╯
  o  4a2df7238c3b 'A'
  

Check rebasing public changeset

  $ hg pull --config phases.publish=True -q -r 6 . # update phase of 6
  $ hg rebase -d 'desc(A)' -b 'desc(C)'
  nothing to rebase
  $ hg debugmakepublic 'desc(C)'
  $ hg rebase -d 'desc(H)' -r 'desc(C)'
  abort: can't rebase public changeset f838bfaca5c7
  (see 'hg help phases' for details)
  [255]
  $ hg rebase -d 'desc(H)' -r 'desc(B) + (desc(C)::)'
  abort: can't rebase public changeset 27547f69f254
  (see 'hg help phases' for details)
  [255]

  $ hg rebase -d 'desc(H)' -b 'desc(C)' --keep
  rebasing 27547f69f254 "B"
  rebasing f838bfaca5c7 "C" (public/f838bfaca5c7226600ebcfd84f3c3c13a28d3757)
  note: not rebasing b3325c91a4d9 "D" and its descendants as this would cause divergence
  rebasing 7d3e5262cbd7 "C" (public/7d3e5262cbd7c2f6fd18aae7d9373efdc84c9d6b)
  rebasing 3f51ceb7b044 "D"

Check rebasing mutable changeset
Source phase greater or equal to destination phase: new changeset get the phase of source:
  $ hg rebase -s'max(desc(D))' -d'desc(A)'
  rebasing 75424903c9c5 "D"
  $ hg log --template "{phase}\n" -r 'max(desc(D))'
  draft
  $ hg rebase -s'max(desc(D))' -d'desc(B)'
  rebasing 90253e7fd6aa "D"
  $ hg log --template "{phase}\n" -r 'max(desc(D))'
  draft
  $ hg rebase -s'max(desc(D))' -d'desc(A)'
  rebasing d757f6e44660 "D"
  $ hg log --template "{phase}\n" -r 'max(desc(D))'
  draft
  $ hg rebase -s'max(desc(D))' -d'desc(B)'
  rebasing 94a187f19238 "D"
  $ hg log --template "{phase}\n" -r 'max(desc(D))'
  draft
Source phase lower than destination phase: new changeset get the phase of destination:
  $ hg rebase -s'max(desc(C))' -d'max(desc(D))'
  rebasing 9ac0d3f9df94 "C"
  $ hg log --template "{phase}\n" -r 'rev(9)'
  draft

  $ cd ..


Test for revset

We need a bit different graph
All destination are B

  $ hg init ah
  $ cd ah
  $ setconfig extensions.treemanifest=$TESTDIR/../edenscm/ext/treemanifestserver.py
  $ setconfig treemanifest.server=True

  $ echo A > A
  $ hg commit -Aqm "A"
  $ echo B > B
  $ hg commit -Aqm "B"
  $ hg up 'desc(A)'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo C > C
  $ hg commit -Aqm "C"
  $ echo D > D
  $ hg commit -Aqm "D"
  $ echo E > E
  $ hg commit -Aqm "E"
  $ echo F > F
  $ hg commit -Aqm "F"
  $ hg up 'desc(D)'
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ echo G > G
  $ hg commit -Aqm "G"
  $ echo H > H
  $ hg commit -Aqm "H"
  $ echo I > I
  $ hg commit -Aqm "I"

  $ tglog
  @  b4b8c20ca6ab 'I'
  │
  o  7603f67b15cc 'H'
  │
  o  15356cd3838c 'G'
  │
  │ o  f1c425d20bac 'F'
  │ │
  │ o  e112fea37bd6 'E'
  ├─╯
  o  36784c4b0d11 'D'
  │
  o  c5cefa58fd55 'C'
  │
  │ o  27547f69f254 'B'
  ├─╯
  o  4a2df7238c3b 'A'
  
  $ cd ..


Simple case with keep:

Source on have two descendant heads but ask for one

  $ hg clone -q -u . ssh://user@dummy/ah ah1
  $ cd ah1
  $ hg rebase -r 'max(desc(C))::desc(I)' -d 'desc(B)' -k
  rebasing c5cefa58fd55 "C"
  rebasing 36784c4b0d11 "D"
  rebasing 15356cd3838c "G"
  rebasing 7603f67b15cc "H"
  rebasing b4b8c20ca6ab "I"
  $ tglog
  @  14e758467275 'I'
  │
  o  59919f39e719 'H'
  │
  o  5c927a9ef4c3 'G'
  │
  o  9a075f22ae65 'D'
  │
  o  5b9a17fd775a 'C'
  │
  │ o  b4b8c20ca6ab 'I'
  │ │
  │ o  7603f67b15cc 'H'
  │ │
  │ o  15356cd3838c 'G'
  │ │
  │ │ o  f1c425d20bac 'F'
  │ │ │
  │ │ o  e112fea37bd6 'E'
  │ ├─╯
  │ o  36784c4b0d11 'D'
  │ │
  │ o  c5cefa58fd55 'C'
  │ │
  o │  27547f69f254 'B'
  ├─╯
  o  4a2df7238c3b 'A'
  

  $ cd ..

Base on have one descendant heads we ask for but common ancestor have two

  $ hg clone -q -u . ssh://user@dummy/ah ah2
  $ cd ah2
  $ hg rebase -r 'desc(D)::desc(I)' -d 'desc(B)' --keep
  rebasing 36784c4b0d11 "D"
  rebasing 15356cd3838c "G"
  rebasing 7603f67b15cc "H"
  rebasing b4b8c20ca6ab "I"
  $ tglog
  @  7dccb2c249c2 'I'
  │
  o  4b72b91391da 'H'
  │
  o  f3f93c3518b4 'G'
  │
  o  a3a79b833e81 'D'
  │
  │ o  b4b8c20ca6ab 'I'
  │ │
  │ o  7603f67b15cc 'H'
  │ │
  │ o  15356cd3838c 'G'
  │ │
  │ │ o  f1c425d20bac 'F'
  │ │ │
  │ │ o  e112fea37bd6 'E'
  │ ├─╯
  │ o  36784c4b0d11 'D'
  │ │
  │ o  c5cefa58fd55 'C'
  │ │
  o │  27547f69f254 'B'
  ├─╯
  o  4a2df7238c3b 'A'
  

  $ cd ..

rebase subset

  $ hg clone -q -u . ssh://user@dummy/ah ah3
  $ cd ah3
  $ hg rebase -r 'desc(D)::desc(H)' -d 'desc(B)' --keep
  rebasing 36784c4b0d11 "D"
  rebasing 15356cd3838c "G"
  rebasing 7603f67b15cc "H"
  $ tglog
  o  4b72b91391da 'H'
  │
  o  f3f93c3518b4 'G'
  │
  o  a3a79b833e81 'D'
  │
  │ @  b4b8c20ca6ab 'I'
  │ │
  │ o  7603f67b15cc 'H'
  │ │
  │ o  15356cd3838c 'G'
  │ │
  │ │ o  f1c425d20bac 'F'
  │ │ │
  │ │ o  e112fea37bd6 'E'
  │ ├─╯
  │ o  36784c4b0d11 'D'
  │ │
  │ o  c5cefa58fd55 'C'
  │ │
  o │  27547f69f254 'B'
  ├─╯
  o  4a2df7238c3b 'A'
  

  $ cd ..

rebase subset with multiple head

  $ hg clone -q -u . ssh://user@dummy/ah ah4
  $ cd ah4
  $ hg rebase -r 'desc(D)::(desc(H)+desc(F))' -d 'desc(B)' --keep
  rebasing 36784c4b0d11 "D"
  rebasing e112fea37bd6 "E"
  rebasing f1c425d20bac "F"
  rebasing 15356cd3838c "G"
  rebasing 7603f67b15cc "H"
  $ tglog
  o  4b72b91391da 'H'
  │
  o  f3f93c3518b4 'G'
  │
  │ o  0080ca510e8b 'F'
  │ │
  │ o  4e2550095559 'E'
  ├─╯
  o  a3a79b833e81 'D'
  │
  │ @  b4b8c20ca6ab 'I'
  │ │
  │ o  7603f67b15cc 'H'
  │ │
  │ o  15356cd3838c 'G'
  │ │
  │ │ o  f1c425d20bac 'F'
  │ │ │
  │ │ o  e112fea37bd6 'E'
  │ ├─╯
  │ o  36784c4b0d11 'D'
  │ │
  │ o  c5cefa58fd55 'C'
  │ │
  o │  27547f69f254 'B'
  ├─╯
  o  4a2df7238c3b 'A'
  

  $ cd ..

More advanced tests

rebase on ancestor with revset

  $ hg clone -q -u . ssh://user@dummy/ah ah5
  $ cd ah5
  $ hg rebase -r 'desc(G)::' -d 'desc(C)'
  rebasing 15356cd3838c "G"
  rebasing 7603f67b15cc "H"
  rebasing b4b8c20ca6ab "I"
  $ tglog
  @  546b91480957 'I'
  │
  o  2d6c4405b0a6 'H'
  │
  o  f382fd7362d5 'G'
  │
  │ o  f1c425d20bac 'F'
  │ │
  │ o  e112fea37bd6 'E'
  │ │
  │ o  36784c4b0d11 'D'
  ├─╯
  o  c5cefa58fd55 'C'
  │
  │ o  27547f69f254 'B'
  ├─╯
  o  4a2df7238c3b 'A'
  
  $ cd ..


rebase with multiple root.
We rebase E and G on B
We would expect heads are I, F if it was supported

  $ hg clone -q -u . ssh://user@dummy/ah ah6
  $ cd ah6
  $ hg rebase -r '(desc(C)+desc(G))::' -d 'desc(B)'
  rebasing c5cefa58fd55 "C"
  rebasing 36784c4b0d11 "D"
  rebasing e112fea37bd6 "E"
  rebasing f1c425d20bac "F"
  rebasing 15356cd3838c "G"
  rebasing 7603f67b15cc "H"
  rebasing b4b8c20ca6ab "I"
  $ tglog
  @  14e758467275 'I'
  │
  o  59919f39e719 'H'
  │
  o  5c927a9ef4c3 'G'
  │
  │ o  edd10be25ef8 'F'
  │ │
  │ o  71b621f0015d 'E'
  ├─╯
  o  9a075f22ae65 'D'
  │
  o  5b9a17fd775a 'C'
  │
  o  27547f69f254 'B'
  │
  o  4a2df7238c3b 'A'
  
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
  @  2fb3a0db434a 'K'
  │
  o  f0001bb73b26 'J'
  │
  │ o  bda6f7e0a0ec 'I'
  │ │
  │ o  15ed2d917603 'H'
  │ │
  │ │ o  3dbfcf9931fb 'G'
  ╭─┬─╯
  │ o  c137c2b8081f 'F'
  │ │
  o │  4e18486b3568 'E'
  ├─╯
  │ o  b3325c91a4d9 'D'
  │ │
  │ o  f838bfaca5c7 'C'
  │ │
  │ o  27547f69f254 'B'
  ├─╯
  o  4a2df7238c3b 'A'
  
(actual test)

  $ hg rebase --dest 'desc(G)' --rev 'desc(K) + desc(I)'
  rebasing bda6f7e0a0ec "I"
  rebasing 2fb3a0db434a "K"
  $ hg log --rev 'children(desc(G))'
  commit:      786f4bf6631d
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     I
  
  commit:      691bd2b0d622
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     K
  
  $ tglog
  @  691bd2b0d622 'K'
  │
  │ o  786f4bf6631d 'I'
  ├─╯
  │ o  f0001bb73b26 'J'
  │ │
  │ │ o  15ed2d917603 'H'
  │ │ │
  o │ │  3dbfcf9931fb 'G'
  ╰─┬─╮
    │ o  c137c2b8081f 'F'
    │ │
    o │  4e18486b3568 'E'
    ├─╯
  o │  b3325c91a4d9 'D'
  │ │
  o │  f838bfaca5c7 'C'
  │ │
  o │  27547f69f254 'B'
  ├─╯
  o  4a2df7238c3b 'A'
  

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
  rebasing * "second source with subdir" (glob)

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
  @  124bb27b6f28 'G'
  │
  │ o  412b391de760 'F'
  │ │
  │ │ o  82ae8dc7a9b7 'E'
  │ │ │
  │ o │  ab709c9f7171 'D'
  │ │ │
  │ │ o  d84f5cfaaf14 'C'
  │ ├─╯
  │ o  76035bbd54bd 'B'
  ├─╯
  o  216878401574 'A'
  

  $ hg rebase -s 'desc(B)' -d 'desc(G)'
  rebasing 76035bbd54bd "B"
  rebasing d84f5cfaaf14 "C"
  rebasing 82ae8dc7a9b7 "E"
  rebasing ab709c9f7171 "D"
  rebasing 412b391de760 "F"

  $ tglog
  o  31884cfb735e 'F'
  │
  o  6d89fa5b0909 'D'
  │
  │ o  de64d97c697b 'E'
  │ │
  │ o  b18e4d2d0aa1 'C'
  ├─╯
  o  0983daf9ff6a 'B'
  │
  @  124bb27b6f28 'G'
  │
  o  216878401574 'A'
  

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
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     aaa
  │
  │ o  commit:      * (glob)
  │ │  user:        test
  │ │  date:        Thu Jan 01 00:00:00 1970 +0000
  │ │  summary:     second source with subdir
  │ │
  │ o  commit:      82901330b6ef
  ├─╯  user:        test
  │    date:        Thu Jan 01 00:00:00 1970 +0000
  │    summary:     first source commit
  │
  o  commit:      58d79cc1cf43
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     dest commit
  │
  o  commit:      e94b687f7da3
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     initial commit
  

Testing from lower head

  $ hg up 'desc(second)'
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg log -r '_destrebase()'
  commit:      5f7bc9025ed2
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     aaa
  

Testing from upper head

  $ hg log -r '_destrebase(desc(aaa))'
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     second source with subdir
  
  $ hg up 'desc(aaa)'
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg log -r '_destrebase()'
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     second source with subdir
  
Testing rebase being called inside another transaction

  $ cd $TESTTMP
  $ hg init tr-state
  $ cd tr-state
  $ cat > $TESTTMP/wraprebase.py <<EOF
  > from __future__ import absolute_import
  > from edenscm import extensions
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
