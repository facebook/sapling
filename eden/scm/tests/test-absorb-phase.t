#debugruntest-compatible
# coding=utf-8
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

  $ cat >> $HGRCPATH << 'EOF'
  > [extensions]
  > absorb=
  > EOF

  $ hg init repo
  $ cd repo
  $ hg debugdrawdag << 'EOS'
  > C
  > |
  > B
  > |
  > A
  > EOS

  $ hg debugmakepublic -r A

  $ hg goto C -q
  $ printf B1 > B

  $ hg absorb -aq

  $ hg log -G -T '{desc} {phase}'
  @  C draft
  │
  o  B draft
  │
  o  A public
