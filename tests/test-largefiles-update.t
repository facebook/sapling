This file focuses mainly on updating largefiles in the working
directory (and ".hg/largefiles/dirstate")

  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > merge = internal:fail
  > [extensions]
  > largefiles =
  > EOF

  $ hg init repo
  $ cd repo

  $ echo large1 > large1
  $ echo large2 > large2
  $ hg add --large large1 large2
  $ echo normal1 > normal1
  $ hg add normal1
  $ hg commit -m '#0'
  $ echo 'large1 in #1' > large1
  $ echo 'normal1 in #1' > normal1
  $ hg commit -m '#1'
  $ hg update -q -C 0
  $ echo 'large2 in #2' > large2
  $ hg commit -m '#2'
  created new head

Test that "hg merge" updates largefiles from "other" correctly

(getting largefiles from "other" normally)

  $ hg status -A large1
  C large1
  $ cat large1
  large1
  $ cat .hglf/large1
  4669e532d5b2c093a78eca010077e708a071bb64
  $ hg merge --config debug.dirstate.delaywrite=2
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  getting changed largefiles
  1 largefiles updated, 0 removed
  $ hg status -A large1
  M large1
  $ cat large1
  large1 in #1
  $ cat .hglf/large1
  58e24f733a964da346e2407a2bee99d9001184f5
  $ hg diff -c 1 --nodates .hglf/large1 | grep '^[+-][0-9a-z]'
  -4669e532d5b2c093a78eca010077e708a071bb64
  +58e24f733a964da346e2407a2bee99d9001184f5

(getting largefiles from "other" via conflict prompt)

  $ hg update -q -C 2
  $ echo 'large1 in #3' > large1
  $ echo 'normal1 in #3' > normal1
  $ hg commit -m '#3'
  $ cat .hglf/large1
  e5bb990443d6a92aaf7223813720f7566c9dd05b
  $ hg merge --config debug.dirstate.delaywrite=2 --config ui.interactive=True <<EOF
  > o
  > EOF
  largefile large1 has a merge conflict
  ancestor was 4669e532d5b2c093a78eca010077e708a071bb64
  keep (l)ocal e5bb990443d6a92aaf7223813720f7566c9dd05b or
  take (o)ther 58e24f733a964da346e2407a2bee99d9001184f5? merging normal1
  warning: conflicts during merge.
  merging normal1 incomplete! (edit conflicts, then use 'hg resolve --mark')
  0 files updated, 1 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  getting changed largefiles
  1 largefiles updated, 0 removed
  [1]
  $ hg status -A large1
  M large1
  $ cat large1
  large1 in #1
  $ cat .hglf/large1
  58e24f733a964da346e2407a2bee99d9001184f5

Test that "hg revert -r REV" updates largefiles from "REV" correctly

  $ hg update -q -C 3
  $ hg status -A large1
  C large1
  $ cat large1
  large1 in #3
  $ cat .hglf/large1
  e5bb990443d6a92aaf7223813720f7566c9dd05b
  $ hg diff -c 1 --nodates .hglf/large1 | grep '^[+-][0-9a-z]'
  -4669e532d5b2c093a78eca010077e708a071bb64
  +58e24f733a964da346e2407a2bee99d9001184f5
  $ hg revert --no-backup -r 1 --config debug.dirstate.delaywrite=2 large1
  $ hg status -A large1
  M large1
  $ cat large1
  large1 in #1
  $ cat .hglf/large1
  58e24f733a964da346e2407a2bee99d9001184f5

Test that "hg rollback" restores status of largefiles correctly

  $ hg update -C -q
  $ hg remove large1
  $ test -f .hglf/large1
  [1]
  $ hg forget large2
  $ test -f .hglf/large2
  [1]
  $ echo largeX > largeX
  $ hg add --large largeX
  $ cat .hglf/largeX
  
  $ hg commit -m 'will be rollback-ed soon'
  $ echo largeY > largeY
  $ hg add --large largeY
  $ hg status -A large1
  large1: No such file or directory
  $ hg status -A large2
  ? large2
  $ hg status -A largeX
  C largeX
  $ hg status -A largeY
  A largeY
  $ hg rollback
  repository tip rolled back to revision 3 (undo commit)
  working directory now based on revision 3
  $ hg status -A large1
  R large1
  $ test -f .hglf/large1
  [1]
  $ hg status -A large2
  R large2
  $ test -f .hglf/large2
  [1]
  $ hg status -A largeX
  A largeX
  $ cat .hglf/largeX
  
  $ hg status -A largeY
  ? largeY
  $ test -f .hglf/largeY
  [1]

Test that "hg rollback" restores standins correctly

  $ hg commit -m 'will be rollback-ed soon'
  $ hg update -q -C 2
  $ cat large1
  large1
  $ cat .hglf/large1
  4669e532d5b2c093a78eca010077e708a071bb64
  $ cat large2
  large2 in #2
  $ cat .hglf/large2
  3cfce6277e7668985707b6887ce56f9f62f6ccd9

  $ hg rollback -q -f
  $ cat large1
  large1
  $ cat .hglf/large1
  4669e532d5b2c093a78eca010077e708a071bb64
  $ cat large2
  large2 in #2
  $ cat .hglf/large2
  3cfce6277e7668985707b6887ce56f9f62f6ccd9

(rollback the parent of the working directory, when the parent of it
is not branch-tip)

  $ hg update -q -C 1
  $ cat .hglf/large1
  58e24f733a964da346e2407a2bee99d9001184f5
  $ cat .hglf/large2
  1deebade43c8c498a3c8daddac0244dc55d1331d

  $ echo normalX > normalX
  $ hg add normalX
  $ hg commit -m 'will be rollback-ed soon'
  $ hg rollback -q

  $ cat .hglf/large1
  58e24f733a964da346e2407a2bee99d9001184f5
  $ cat .hglf/large2
  1deebade43c8c498a3c8daddac0244dc55d1331d

Test that "hg status" shows status of largefiles correctly just after
automated commit like rebase/transplant

  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > rebase =
  > strip =
  > transplant =
  > EOF
  $ hg update -q -C 1
  $ hg remove large1
  $ echo largeX > largeX
  $ hg add --large largeX
  $ hg commit -m '#4'

  $ hg rebase -s 1 -d 2 --keep
  $ hg status -A large1
  large1: No such file or directory
  $ hg status -A largeX
  C largeX
  $ hg strip -q 5

  $ hg update -q -C 2
  $ hg transplant -q 1 4
  $ hg status -A large1
  large1: No such file or directory
  $ hg status -A largeX
  C largeX
  $ hg strip -q 5

  $ hg update -q -C 2
  $ hg transplant -q --merge 1 --merge 4
  $ hg status -A large1
  large1: No such file or directory
  $ hg status -A largeX
  C largeX
  $ hg strip -q 5

Test that linear merge can detect modification (and conflict) correctly

(linear merge without conflict)

  $ echo 'large2 for linear merge (no conflict)' > large2
  $ hg update 3 --config debug.dirstate.delaywrite=2
  getting changed largefiles
  1 largefiles updated, 0 removed
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg status -A large2
  M large2
  $ cat large2
  large2 for linear merge (no conflict)
  $ cat .hglf/large2
  9c4bf8f1b33536d6e5f89447e10620cfe52ea710

(linear merge with conflict, choosing "other")

  $ hg update -q -C 2
  $ echo 'large1 for linear merge (conflict)' > large1
  $ hg update 3 --config ui.interactive=True <<EOF
  > o
  > EOF
  largefile large1 has a merge conflict
  ancestor was 4669e532d5b2c093a78eca010077e708a071bb64
  keep (l)ocal ba94c2efe5b7c5e0af8d189295ce00553b0612b7 or
  take (o)ther e5bb990443d6a92aaf7223813720f7566c9dd05b? getting changed largefiles
  1 largefiles updated, 0 removed
  1 files updated, 1 files merged, 0 files removed, 0 files unresolved
  $ hg status -A large1
  C large1
  $ cat large1
  large1 in #3
  $ cat .hglf/large1
  e5bb990443d6a92aaf7223813720f7566c9dd05b

(linear merge with conflict, choosing "local")

  $ hg update -q -C 2
  $ echo 'large1 for linear merge (conflict)' > large1
  $ hg update 3 --config debug.dirstate.delaywrite=2
  largefile large1 has a merge conflict
  ancestor was 4669e532d5b2c093a78eca010077e708a071bb64
  keep (l)ocal ba94c2efe5b7c5e0af8d189295ce00553b0612b7 or
  take (o)ther e5bb990443d6a92aaf7223813720f7566c9dd05b? l
  1 files updated, 1 files merged, 0 files removed, 0 files unresolved
  $ hg status -A large1
  M large1
  $ cat large1
  large1 for linear merge (conflict)
  $ cat .hglf/large1
  ba94c2efe5b7c5e0af8d189295ce00553b0612b7

Test a linear merge to a revision containing same-name normal file

  $ hg update -q -C 3
  $ hg remove large2
  $ echo 'large2 as normal file' > large2
  $ hg add large2
  $ echo 'large3 as normal file' > large3
  $ hg add large3
  $ hg commit -m '#5'
  $ hg manifest
  .hglf/large1
  large2
  large3
  normal1

(modified largefile is already switched to normal)

  $ hg update -q -C 2
  $ echo 'modified large2 for linear merge' > large2
  $ hg update -q 5
  local changed .hglf/large2 which remote deleted
  use (c)hanged version or (d)elete? c
  remote turned local largefile large2 into a normal file
  keep (l)argefile or use (n)ormal file? l
  $ hg debugdirstate --nodates | grep large2
  a   0         -1 .hglf/large2
  r   0          0 large2
  $ hg status -A large2
  A large2
  $ cat large2
  modified large2 for linear merge

(added largefile is already committed as normal)

  $ hg update -q -C 2
  $ echo 'large3 as large file for linear merge' > large3
  $ hg add --large large3
  $ hg update -q 5
  remote turned local largefile large3 into a normal file
  keep (l)argefile or use (n)ormal file? l
  $ hg debugdirstate --nodates | grep large3
  a   0         -1 .hglf/large3
  r   0          0 large3
  $ hg status -A large3
  A large3
  $ cat large3
  large3 as large file for linear merge

  $ cd ..
