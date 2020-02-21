# coding=utf-8
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


feature.require(["py2"])


feature.require(["no-windows", "no-osx"])

# Test that trying to add invalid utf8 files to the repository will fail.

sh % "hg init"
open("\x9d\xc8\xac\xde\xa1\xee", "w").write("test")

sh % "hg status" == "skipping invalid utf-8 filename: '\x9d\xc8\xac\xde\xa1\xee'"

# fsmonitor ignores the file once, so it has slightly different output from here
if feature.check("fsmonitor"):
    sh % "hg addremove" == ''
    sh % "hg commit -m 'adding a filename that is invalid utf8'" == r"""
        nothing changed
        [1]"""
else:
    sh % "hg addremove" == "skipping invalid utf-8 filename: '\x9d\xc8\xac\xde\xa1\xee'"
    sh % "hg commit -m 'adding a filename that is invalid utf8'" == """
        skipping invalid utf-8 filename: '\x9d\xc8\xac\xde\xa1\xee'
        skipping invalid utf-8 filename: '\x9d\xc8\xac\xde\xa1\xee'
        nothing changed
        [1]"""
