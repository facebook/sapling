#debugruntest-compatible
# coding=utf-8
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Test bookmark -D

  $ setconfig devel.segmented-changelog-rev-compat=true
  $ hg init book-D
  $ cd book-D
  $ cat >> .hg/hgrc << 'EOF'
  > [extensions]
  > amend=
  > tweakdefaults=
  > [experimental]
  > evolution=all
  > EOF
  $ hg debugbuilddag '+4*2*2*2'
  $ hg bookmark -i -r 1 master
  $ hg bookmark -i -r 5 feature1
  $ hg bookmark -i -r 6 feature2
  $ hg log -G -T '{rev} {bookmarks}' -r 'all()'
  o  6 feature2
  │
  │ o  5 feature1
  │ │
  o │  4
  │ │
  │ o  3
  ├─╯
  o  2
  │
  o  1 master
  │
  o  0
  $ hg bookmark -D feature1
  hiding commit 2dc09a01254d "r3"
  hiding commit 191de46dc8b9 "r5"
  2 changesets hidden
  removing bookmark 'feature1' (was at: 191de46dc8b9)
  1 bookmark removed
  $ hg log -G -T '{rev} {bookmarks}' -r 'all()' --hidden
  o  6 feature2
  │
  │ o  5
  │ │
  o │  4
  │ │
  │ o  3
  ├─╯
  o  2
  │
  o  1 master
  │
  o  0
