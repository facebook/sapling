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

  $ cd ..
