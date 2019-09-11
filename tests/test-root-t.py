# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


# make shared repo

sh % "enable share"
sh % "newrepo repo1"
sh % "echo a" > "a"
sh % "hg commit -q -A -m init"
sh % "cd '$TESTTMP'"
sh % "hg share -q repo1 repo2"
sh % "cd repo2"

# test root

sh % "hg root" == "$TESTTMP/repo2"

# test root --shared

sh % "hg root --shared" == "$TESTTMP/repo1"

# test error message

sh % "hg root --cwd .." == r"""
    abort: '$TESTTMP' is not inside a repository, but this command requires a repository!
    (use 'cd' to go to a directory inside a repository and try again)
    [255]"""
