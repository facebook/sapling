
#require no-eden

# Copyright (c) Meta Platforms, Inc. and affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

  $ eagerepo
  $ hg init repo
  $ cd repo
  $ touch unknown

  $ touch a
  $ hg add a
  $ hg ci -m initial

  $ touch b
  $ hg add b
  $ hg ci -m second

# Should show unknown

  $ hg status
  ? unknown
  $ hg revert -r 'desc(initial)' --all
  removing b

# Should show unknown and b removed

  $ hg status
  R b
  ? unknown

# Should show a and unknown

  $ ls
  a
  unknown
