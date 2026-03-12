
#require no-eden

# Copyright (c) Meta Platforms, Inc. and affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

  $ eagerepo

  $ hg init a
  $ cd a
  $ hg init b
  $ hg st

Fsmonitor doesn't handle nested repos well, but the above test shows we at least don't
consider files under the nested ".hg" directory.
#if no-fsmonitor

  $ echo a > a
  $ hg ci -Ama a
  $ echo x > b/x

# Should print nothing:

  $ hg add b
  $ hg st

  $ echo y > b/y
  $ hg st

# These should ideally fail, although not failing is not causing security issues:

  $ hg add b/x
  $ hg add b b/x
  $ hg mv a b

  $ cd ..

#endif
