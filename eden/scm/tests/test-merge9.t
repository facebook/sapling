#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

# test that we don't interrupt the merge session if
# a file-level merge failed

  $ setconfig devel.segmented-changelog-rev-compat=true
#if fsmonitor
  $ setconfig workingcopy.ruststatus=False
#endif

  $ hg init repo
  $ cd repo

  $ echo foo > foo
  $ echo a > bar
  $ hg ci -Am 'add foo'
  adding bar
  adding foo

  $ hg mv foo baz
  $ echo b >> bar
  $ echo quux > quux1
  $ hg ci -Am 'mv foo baz'
  adding quux1

  $ hg up -qC 0
  $ echo >> foo
  $ echo c >> bar
  $ echo quux > quux2
  $ hg ci -Am 'change foo'
  adding quux2

# test with the rename on the remote side

  $ HGMERGE=false hg merge
  merging bar
  merging foo and baz to baz
  merging bar failed!
  1 files updated, 1 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]
  $ hg resolve -l
  U bar
  R baz

# test with the rename on the local side

  $ hg up -C 1
  3 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ HGMERGE=false hg merge
  merging bar
  merging baz and foo to baz
  merging bar failed!
  1 files updated, 1 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]

# show unresolved

  $ hg resolve -l
  U bar
  R baz

# unmark baz

  $ hg resolve -u baz

# show

  $ hg resolve -l
  U bar
  U baz
  $ hg st
  M bar
  M baz
  M quux2
  ? bar.orig

# re-resolve baz

  $ hg resolve baz
  merging baz and foo to baz

# after resolve

  $ hg resolve -l
  U bar
  R baz

# resolve all warning

  $ hg resolve
  abort: no files or directories specified
  (use --all to re-merge all unresolved files)
  [255]

# resolve all

  $ hg resolve -a
  merging bar
  warning: 1 conflicts while merging bar! (edit, then use 'hg resolve --mark')
  [1]

# after

  $ hg resolve -l
  U bar
  R baz

  $ cd ..
