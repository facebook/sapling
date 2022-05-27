#debugruntest-compatible
# coding=utf-8

# coding=utf-8

# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

  $ newrepo
  $ hg debugdrawdag << 'EOS'
  > J K
  > |/|
  > H I
  > | |
  > F G
  > |/
  > E
  > |\
  > A D
  > |\|
  > B C
  > EOS

  $ hg debugbindag -r '::A' -o a.dag
  $ hg debugpreviewbindag a.dag
  o    2
  ├─╮
  o │  1
    │
    o  0

  $ hg debugbindag -r '::J' -o j.dag
  $ hg debugpreviewbindag j.dag
  o  7
  │
  o  6
  │
  o  5
  │
  o    4
  ├─╮
  │ o  3
  │ │
  o │  2
  ├─╮
  │ o  1
  │
  o  0

  $ hg debugbindag -r 'all()' -o all.dag
  $ hg debugpreviewbindag all.dag
  o    10
  ├─╮
  │ │ o  9
  │ ├─╯
  o │  8
  │ │
  │ o  7
  │ │
  o │  6
  │ │
  │ o  5
  ├─╯
  o    4
  ├─╮
  │ o  3
  │ │
  o │  2
  ├─╮
  │ o  1
  │
  o  0
