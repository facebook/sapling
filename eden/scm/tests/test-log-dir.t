#debugruntest-compatible
# coding=utf-8

# coding=utf-8

# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

  $ newrepo
  $ drawdag << 'EOS'
  > C   # C/x/3=3
  > | D # C/x/2=2
  > |/  # D/x/4=4
  > B
  > |
  > A   # A/x/1=1
  > EOS

  $ hg goto -q $C

# Log a directory:

  $ hg log -T '{desc}\n' -f x
  C
  A

# From non-repo root:

  $ cd x
  $ hg log -G -T '{desc}\n' -f .
  @  C
  ╷
  o  A

# Using the follow revset, which is related to repo root:

  $ hg log -G -T '{desc}\n' -r 'follow("x")'
  @  C
  ╷
  o  A
  $ hg log -G -T '{desc}\n' -r 'follow(".")'
  @  C
  │
  o  B
  │
  o  A
  $ hg log -G -T '{desc}\n' -r 'follow("relpath:.")'
  @  C
  ╷
  o  A
