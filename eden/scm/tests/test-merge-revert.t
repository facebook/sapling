#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

  $ setconfig devel.segmented-changelog-rev-compat=true
  $ hg init repo
  $ cd repo

  $ echo 'added file1' > file1
  $ echo 'added file2' > file2
  $ hg add file1 file2
  $ hg commit -m 'added file1 and file2'

  $ echo 'changed file1' >> file1
  $ hg commit -m 'changed file1'

  $ hg -q log
  08a16e8e4408
  d29c767a4b52
  $ hg id
  08a16e8e4408

  $ hg goto -C 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg id
  d29c767a4b52
  $ echo 'changed file1' >> file1
  $ hg id
  d29c767a4b52+

  $ hg revert --all
  reverting file1
  $ hg diff
  $ hg status
  ? file1.orig
  $ hg id
  d29c767a4b52

  $ hg goto
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg diff
  $ hg status
  ? file1.orig
  $ hg id
  08a16e8e4408

  $ hg goto -C 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo 'changed file1' >> file1

  $ hg goto
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg diff
  $ hg status
  ? file1.orig
  $ hg id
  08a16e8e4408

  $ hg revert --all
  $ hg diff
  $ hg status
  ? file1.orig
  $ hg id
  08a16e8e4408

  $ hg revert -r tip --all
  $ hg diff
  $ hg status
  ? file1.orig
  $ hg id
  08a16e8e4408

  $ hg goto -C
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg diff
  $ hg status
  ? file1.orig
  $ hg id
  08a16e8e4408
