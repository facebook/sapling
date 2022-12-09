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
  $ echo This is file b1 > b
  $ hg add b
  $ hg commit -m 'commit #1'
  $ hg goto 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo This is file c1 > c
  $ hg add c
  $ hg commit -m 'commit #2'
  $ hg merge 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ rm b
  $ echo This is file c22 > c

# Test hg behaves when committing with a missing file added by a merge

  $ hg commit -m 'commit #3'
  abort: cannot commit merge with missing files
  [255]
