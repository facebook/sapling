#debugruntest-compatible
# coding=utf-8

# coding=utf-8

# Copyright (c) Meta Platforms, Inc. and affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

# This test file aims at test topological iteration and the various configuration it can has.

  $ setconfig devel.segmented-changelog-rev-compat=true
  $ cat >> $HGRCPATH << 'EOF'
  > [ui]
  > logtemplate={rev}\n
  > allowemptycommit=True
  > EOF

# On this simple example, all topological branch are displayed in turn until we
# can finally display 0. this implies skipping from 8 to 3 and coming back to 7
# later.

  $ hg init test01
  $ cd test01
  $ hg commit -qm 0
  $ hg commit -qm 1
  $ hg commit -qm 2
  $ hg commit -qm 3
  $ hg up -q 0
  $ hg commit -qm 4
  $ hg commit -qm 5
  $ hg commit -qm 6
  $ hg commit -qm 7
  $ hg up -q 3
  $ hg commit -qm 8
  $ hg up -q null

  $ hg log -G
  o  8
  │
  │ o  7
  │ │
  │ o  6
  │ │
  │ o  5
  │ │
  │ o  4
  │ │
  o │  3
  │ │
  o │  2
  │ │
  o │  1
  ├─╯
  o  0

# (display all nodes)

  $ hg log -G -r 'sort(all(), topo)'
  o  8
  │
  o  3
  │
  o  2
  │
  o  1
  │
  │ o  7
  │ │
  │ o  6
  │ │
  │ o  5
  │ │
  │ o  4
  ├─╯
  o  0

# (revset skipping nodes)

  $ hg log -G --rev 'sort(not (2+6), topo)'
  o  8
  │
  o  3
  ╷
  o  1
  │
  │ o  7
  │ ╷
  │ o  5
  │ │
  │ o  4
  ├─╯
  o  0

# (begin) from the other branch

  $ hg log -G -r 'sort(all(), topo, topo.firstbranch=5)'
  o  7
  │
  o  6
  │
  o  5
  │
  o  4
  │
  │ o  8
  │ │
  │ o  3
  │ │
  │ o  2
  │ │
  │ o  1
  ├─╯
  o  0
