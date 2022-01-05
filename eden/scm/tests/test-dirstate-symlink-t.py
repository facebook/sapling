# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


feature.require(["symlink"])

sh % "cd $TESTTMP"

sh % "newrepo"
sh % "mkdir a b"
sh % "touch a/x"

sh % "hg ci -m init -A a/x"

# Replace the directory with a symlink

sh % "mv a/x b/x"
sh % "rmdir a"
sh % "ln -s b a"

# "! a/x" should be shown, as it is implicitly removed

sh % "hg status" == r"""
    ! a/x
    ? a
    ? b/x"""

sh % "hg ci -m rename -A ." == r"""
    adding a
    removing a/x
    adding b/x"""

# "a/x" should not show up in "hg status", even if it exists

sh % "hg status"
