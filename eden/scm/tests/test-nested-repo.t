#debugruntest-compatible
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

  $ echo x > b/x

# Should print nothing:

  $ hg add b
  $ hg st

  $ echo y > b/y
  $ hg st

# Should fail:

  $ hg add b/x
  abort: path 'b/x' is inside nested repo 'b'
  [255]

# Should fail:

  $ hg add b b/x
  abort: path 'b/x' is inside nested repo 'b'
  [255]
  $ hg st

# Should arguably print nothing:

  $ hg st b

  $ echo a > a
  $ hg ci -Ama a

# Should fail:

  $ hg mv a b
  abort: path 'b/a' is inside nested repo 'b'
  [255]
  $ hg st

  $ cd ..

#endif
