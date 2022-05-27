#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

# https://bz.mercurial-scm.org/1089

  $ hg init repo
  $ cd repo
  $ mkdir a
  $ echo a > a/b
  $ hg ci -Am m
  adding a/b

  $ hg rm a
  removing a/b
  $ hg ci -m m a

  $ mkdir a b
  $ echo a > a/b
  $ hg ci -Am m
  adding a/b

  $ hg rm a
  removing a/b
  $ cd b

# Relative delete:

  $ hg ci -m m ../a

  $ cd ..
