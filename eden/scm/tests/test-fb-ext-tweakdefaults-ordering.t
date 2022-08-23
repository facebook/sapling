#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# TODO: Make this test compatibile with obsstore enabled.

  $ setconfig 'experimental.evolution='

# Set up extensions (order is important here, we must test tweakdefaults loading last)

  $ cat >> $HGRCPATH << 'EOF'
  > [extensions]
  > rebase=
  > remotenames=
  > tweakdefaults=
  > EOF

# Run test

  $ hg init repo
  $ cd repo
  $ touch a
  $ hg commit -Aqm a
  $ touch b
  $ hg commit -Aqm b
  $ hg bookmark AB
  $ hg up '.^'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (leaving bookmark AB)
  $ touch c
  $ hg commit -Aqm c
  $ hg bookmark C -t AB
  $ hg rebase
  rebasing d5e255ef74f8 "c" (C)
