#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

# make shared repo

  $ export HGIDENTITY=sl
  $ enable share
  $ newrepo repo1
  $ echo a > a
  $ hg commit -q -A -m init
  $ cd "$TESTTMP"
  $ hg share -q repo1 repo2
  $ cd repo2

# test root

  $ hg root
  $TESTTMP/repo2

  $ hg root --dotdir
  $TESTTMP/repo2/.sl

# test root --shared

  $ hg root --shared
  $TESTTMP/repo1

  $ hg root --shared --dotdir
  $TESTTMP/repo1/.sl

# test error message

  $ hg root --cwd ..
  abort: '$TESTTMP' is not inside a repository, but this command requires a repository!
  (use 'cd' to go to a directory inside a repository and try again)
  [255]
