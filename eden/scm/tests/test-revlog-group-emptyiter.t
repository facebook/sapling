#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

#if fsmonitor
  $ setconfig workingcopy.ruststatus=False
#endif

  $ setconfig experimental.allowfilepeer=True
  $ setconfig 'extensions.treemanifest=!'

# Issue1678: IndexError when pushing
# setting up base repo

  $ hg init a
  $ cd a
  $ touch a
  $ hg ci -Am a
  adding a
  $ cd ..

# cloning base repo

  $ hg clone a b
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd b

# setting up cset to push

  $ hg up null
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ touch a

# different msg so we get a clog new entry

  $ hg ci -Am b
  adding a

# pushing

  $ hg push -f ../a
  pushing to ../a
  searching for changes
  adding changesets
  adding manifests
  adding file changes

  $ cd ..
