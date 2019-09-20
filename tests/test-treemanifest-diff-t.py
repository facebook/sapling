# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


# Setup the repository

sh % "hg init myrepo"
sh % "cd myrepo"
sh % "mkdir -p foo/bar-test foo/bartest"
sh % "echo a" > "foo/bar-test/a.txt"
sh % "echo b" > "foo/bartest/b.txt"
sh % "hg add ." == r"""
    adding foo/bar-test/a.txt
    adding foo/bartest/b.txt"""
sh % "hg commit -m Init"

sh % "mkdir foo/bar"
sh % "echo c" > "foo/bar/c.txt"
sh % "hg add ." == r"""
    adding foo/bar/c.txt"""
sh % "hg commit -m 'Add foo/bar/c.txt'"

sh % "hg diff -r .^ -r . --stat" == r"""
    foo/bar/c.txt |  1 +
    1 files changed, 1 insertions(+), 0 deletions(-)"""
