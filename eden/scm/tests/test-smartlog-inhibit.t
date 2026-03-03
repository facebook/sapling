
#require no-eden

# coding=utf-8
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

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
  $ hg book -r 'desc(r3)' test
  $ hg up 'desc(r1)'
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg amend -m amended --no-rebase
  hint[amend-restack]: descendants of 66f7d451a68b are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints
  $ hg smartlog -T '{desc|firstline} {bookmarks}'
  o  r3 test
  │
  o  r2
  │
  x  r1
  │
  │ @  amended
  ├─╯
  o  r0
  $ hg unamend
  $ hg up 'desc(r2)'
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg debugmakepublic -r .
  $ hg smartlog -T '{desc|firstline} {bookmarks}'
  o  r3 test
  │
  @  r2
  │
  ~
