# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "enable sparse"
sh % "newrepo"
sh % "hg sparse include a/b"
sh % "cat .hg/sparse" == r"""
    [include]
    a/b
    [exclude]"""
sh % "mkdir -p a/b b/c"
sh % "touch a/b/c b/c/d"

sh % "hg status" == "? a/b/c"

# More complex pattern
sh % "hg sparse include ''\\''a*/b*/c'\\'''"
sh % "mkdir -p a1/b1"
sh % "touch a1/b1/c"

sh % "hg status" == r"""
    ? a/b/c
    ? a1/b1/c"""
