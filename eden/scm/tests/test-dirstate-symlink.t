#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

#require symlink

  $ setconfig workingcopy.ruststatus=False

  $ cd $TESTTMP

  $ newrepo
  $ mkdir a b
  $ touch a/x

  $ hg ci -m init -A a/x

# Replace the directory with a symlink

  $ mv a/x b/x
  $ rmdir a
  $ ln -s b a

# "! a/x" should be shown, as it is implicitly removed

  $ hg status
  ! a/x
  ? a
  ? b/x

  $ hg ci -m rename -A .
  adding a
  removing a/x
  adding b/x

# "a/x" should not show up in "hg status", even if it exists

  $ hg status
