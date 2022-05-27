#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

  $ hg init

  $ mkdir alpha
  $ touch alpha/one
  $ mkdir beta
  $ touch beta/two

  $ hg add alpha/one beta/two
  $ hg ci -m start

  $ echo 1 > alpha/one
  $ echo 2 > beta/two

# everything

  $ hg diff --nodates
  diff -r * alpha/one (glob)
  --- a/alpha/one
  +++ b/alpha/one
  @@ -0,0 +1,1 @@
  +1
  diff -r * beta/two (glob)
  --- a/beta/two
  +++ b/beta/two
  @@ -0,0 +1,1 @@
  +2

# beta only

  $ hg diff --nodates beta
  diff -r * beta/two (glob)
  --- a/beta/two
  +++ b/beta/two
  @@ -0,0 +1,1 @@
  +2

# inside beta

  $ cd beta
  $ hg diff --nodates .
  diff -r * beta/two (glob)
  --- a/beta/two
  +++ b/beta/two
  @@ -0,0 +1,1 @@
  +2

# relative to beta

  $ cd ..
  $ hg diff --nodates --root beta
  diff -r * two (glob)
  --- a/two
  +++ b/two
  @@ -0,0 +1,1 @@
  +2

# inside beta

  $ cd beta
  $ hg diff --nodates --root .
  diff -r * two (glob)
  --- a/two
  +++ b/two
  @@ -0,0 +1,1 @@
  +2

  $ cd ..
