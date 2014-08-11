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
  $ hg forget large2
  $ echo largeX > largeX
  $ hg add --large largeX
  $ hg commit -m 'will be rollback-ed soon'
  $ hg status -A large1
  large1: No such file or directory
  $ hg status -A large2
  ? large2
  $ hg status -A largeX
  C largeX
  $ hg rollback
  repository tip rolled back to revision 3 (undo commit)
  working directory now based on revision 3
  $ hg status -A large1
  R large1
  $ hg status -A large2
  R large2
  $ hg status -A largeX
  A largeX

  $ cd ..
