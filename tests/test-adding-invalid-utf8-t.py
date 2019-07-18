# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


feature.require(["no-windows", "no-osx"])

# Test that trying to add invalid utf8 files to the repository will fail.

sh % "hg init"
open("invalid\x80utf8", "w").write("test")
sh % "hg addremove" == "adding invalid\\x80utf8 (esc)"
sh % "hg commit -m 'adding a filename that is invalid utf8'" == r"""
    abort: invalid file name encoding: invalid\x80utf8! (esc)
    [255]"""
