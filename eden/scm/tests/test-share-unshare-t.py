# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


# Share works with blackbox enabled:

sh % "cat" << r"""
[extensions]
blackbox =
share =
""" >> "$HGRCPATH"

sh % "hg init a"
sh % "hg share a b" == r"""
    updating working directory
    0 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "cd b"
sh % "hg unshare"
