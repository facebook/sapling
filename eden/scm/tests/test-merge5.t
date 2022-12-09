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
  $ echo This is file b1 > b
  $ hg add a b
  $ hg commit -m 'commit #0'
  $ echo This is file b22 > b
  $ hg commit -m 'comment #1'
  $ hg goto 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm b
  $ hg commit -A -m 'comment #2'
  removing b
  $ hg goto 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm b
  $ hg goto -c 2
  abort: uncommitted changes
  [255]
  $ hg revert b
  $ hg goto -c 2
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mv a c

# Should abort:

  $ hg goto 1
  abort: uncommitted changes
  (commit or goto --clean to discard changes)
  [255]
  $ mv c a

# Should succeed:

  $ hg goto 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
