#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

  $ setconfig devel.segmented-changelog-rev-compat=true
  $ hg init repo
  $ cd repo
  $ echo This is file a1 > a
  $ hg add a
  $ hg commit -m 'commit #0'
  $ ls
  a
  $ echo This is file b1 > b
  $ hg add b
  $ hg commit -m 'commit #1'
  $ hg co 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved

# B should disappear

  $ ls
  a
