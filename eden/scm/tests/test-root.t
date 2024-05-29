
#require no-eden

# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

# make shared repo

  $ eagerepo
  $ export HGIDENTITY=sl
  $ enable share
  $ newrepo repo1
  $ mkdir dir
  $ echo a > dir/a
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

# Make sure we can find repo when --cwd is symlink into repo.
  $ cd
  $ ln -s repo1/dir my-link
  $ hg root --cwd my-link
  $TESTTMP/repo1

# Don't mess up with symlinks within repo
  $ ln -s $TESTTMP repo1/testtmp
  $ hg root --cwd repo1/testtmp
  $TESTTMP/repo1

