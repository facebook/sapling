#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

  $ setconfig workingcopy.ruststatus=False

  $ enable sparse
  $ newrepo
  $ hg sparse include a/b
  $ cat .hg/sparse
  [include]
  a/b
  [exclude]
  $ mkdir -p a/b b/c
  $ touch a/b/c b/c/d

  $ hg status
  ? a/b/c

# More complex pattern

  $ hg sparse include 'a*/b*/c'
  $ mkdir -p a1/b1
  $ touch a1/b1/c

  $ hg status
  ? a/b/c
  ? a1/b1/c
