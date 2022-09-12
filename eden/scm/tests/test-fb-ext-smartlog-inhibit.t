#debugruntest-compatible
# coding=utf-8
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

  $ setconfig format.use-segmented-changelog=true
  $ setconfig devel.segmented-changelog-rev-compat=true
  $ cat >> $HGRCPATH << 'EOF'
  > [extensions]
  > amend=
  > smartlog=
  > [experimental]
  > evolution = createmarkers
  > EOF

# Test that changesets with visible precursors are rendered as x's

  $ hg init repo
  $ cd repo
  $ hg debugbuilddag +4
  $ hg book -r 3 test
  $ hg up 1
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg amend -m amended --no-rebase
  hint[amend-restack]: descendants of 66f7d451a68b are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints
  $ hg smartlog -T '{rev} {bookmarks}'
  o  3 test
  │
  o  2
  │
  x  1
  │
  │ @  4
  ├─╯
  o  0
  $ hg unamend
  $ hg up 2
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg debugmakepublic -r .
  $ hg smartlog -T '{rev} {bookmarks}'
  o  3 test
  │
  @  2
  │
  ~
