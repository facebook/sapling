#debugruntest-compatible
# coding=utf-8

# Copyright (c) Meta Platforms, Inc. and affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

# TODO: Make this test compatibile with obsstore enabled.

  $ setconfig devel.segmented-changelog-rev-compat=true
  $ setconfig 'experimental.evolution='
  $ cat >> $HGRCPATH << 'EOF'
  > [extensions]
  > rebase=
  > 
  > [phases]
  > publish=False
  > EOF

  $ hg init a
  $ cd a

  $ echo c1 > c1
  $ hg ci -Am c1
  adding c1

  $ echo c2 > c2
  $ hg ci -Am c2
  adding c2

  $ echo l1 > l1
  $ hg ci -Am l1
  adding l1

  $ hg up -q -C 1

  $ echo r1 > r1
  $ hg ci -Am r1
  adding r1

  $ echo r2 > r2
  $ hg ci -Am r2
  adding r2

  $ tglog
  @  225af64d03e6 'r2'
  │
  o  8d0a8c99b309 'r1'
  │
  │ o  87c180a611f2 'l1'
  ├─╯
  o  56daeba07f4b 'c2'
  │
  o  e8faad3d03ff 'c1'

# Rebase with no arguments - single revision in source branch:

  $ hg up -q -C 2

  $ hg rebase
  rebasing 87c180a611f2 "l1"

  $ tglog
  @  b1152cc99655 'l1'
  │
  o  225af64d03e6 'r2'
  │
  o  8d0a8c99b309 'r1'
  │
  o  56daeba07f4b 'c2'
  │
  o  e8faad3d03ff 'c1'
  $ cd ..

  $ hg init b
  $ cd b

  $ echo c1 > c1
  $ hg ci -Am c1
  adding c1

  $ echo c2 > c2
  $ hg ci -Am c2
  adding c2

  $ echo l1 > l1
  $ hg ci -Am l1
  adding l1

  $ echo l2 > l2
  $ hg ci -Am l2
  adding l2

  $ hg up -q -C 1

  $ echo r1 > r1
  $ hg ci -Am r1
  adding r1

  $ tglog
  @  8d0a8c99b309 'r1'
  │
  │ o  1ac923b736ef 'l2'
  │ │
  │ o  87c180a611f2 'l1'
  ├─╯
  o  56daeba07f4b 'c2'
  │
  o  e8faad3d03ff 'c1'

# Rebase with no arguments - single revision in target branch:

  $ hg up -q -C 3

  $ hg rebase
  rebasing 87c180a611f2 "l1"
  rebasing 1ac923b736ef "l2"

  $ tglog
  @  023181307ed0 'l2'
  │
  o  913ab52b43b4 'l1'
  │
  o  8d0a8c99b309 'r1'
  │
  o  56daeba07f4b 'c2'
  │
  o  e8faad3d03ff 'c1'

  $ cd ..
