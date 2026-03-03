
#require no-eden

# coding=utf-8
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Test bookmark -D

  $ eagerepo
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
  $ hg bookmark -i -r 'desc(r1)' master
  $ hg bookmark -i -r 'desc(r5)' feature1
  $ hg bookmark -i -r 'desc(r6)' feature2
  $ hg log -G -T '{desc} {bookmarks}' -r 'all()'
  o  r5 feature1
  │
  o  r3
  │
  │ o  r6 feature2
  │ │
  │ o  r4
  ├─╯
  o  r2
  │
  o  r1 master
  │
  o  r0
  $ hg bookmark -D feature1
  hiding commit 2dc09a01254d "r3"
  hiding commit 191de46dc8b9 "r5"
  2 changesets hidden
  removing bookmark 'feature1' (was at: 191de46dc8b9)
  1 bookmark removed
  $ hg log -G -T '{desc} {bookmarks}' -r 'all()' --hidden
  o  r5
  │
  o  r3
  │
  │ o  r6 feature2
  │ │
  │ o  r4
  ├─╯
  o  r2
  │
  o  r1 master
  │
  o  r0
