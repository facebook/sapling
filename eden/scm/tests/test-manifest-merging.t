#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

  $ configure modernclient

  $ newclientrepo base

  $ echo alpha > alpha
  $ hg ci -A -m 'add alpha'
  adding alpha
  $ hg push -q --to book --create
  $ cd ..

  $ newclientrepo work test:base_server book

  $ echo beta > beta
  $ hg ci -A -m 'add beta'
  adding beta
  $ cd ..

  $ cd base
  $ echo gamma > gamma
  $ hg ci -A -m 'add gamma'
  adding gamma
  $ hg push -q --to book
  $ cd ..

  $ cd work
  $ hg pull -q
  $ hg merge
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

# Update --clean to revision 1 to simulate a failed merge:

  $ rm alpha beta gamma
  $ hg goto --clean 'desc(beta)'
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cd ..
