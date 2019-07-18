# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


feature.require(["symlink"])

sh % "mkdir -p a"
sh % "ln -s a/b a/c"
sh % "hg debugshell -c 'm.util.makedirs(\"a/c/e/f\")'" == r"""
    abort: Symlink '$TESTTMP/a/c' points to non-existed destination 'a/b' during makedir: '$TESTTMP/a/c/e'
    [255]"""
