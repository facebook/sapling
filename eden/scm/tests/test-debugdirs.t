#debugruntest-compatibile
#debugruntest-compatible

# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

  $ newrepo

  $ for d in a a/b a/b/c a/b/d b/c/ b/d; do
  >   mkdir -p $d
  >   touch $d/x
  > done

  $ hg commit -Aqm init

  $ hg debugdirs a a/b a/b/c a/b/d b/c/ b/d m m/n a/b/m b/m/ b/m/n
  a
  a/b
  a/b/c
  a/b/d
  b/c/
  b/d
