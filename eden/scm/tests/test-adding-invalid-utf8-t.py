# coding=utf-8
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


feature.require(["py2"])


feature.require(["no-windows", "no-osx"])

# Test that trying to add invalid utf8 files to the repository will fail.

sh % "hg init"
open("\x9d\xc8\xac\xde\xa1\xee", "wb").write("test")


# fsmonitor ignores the file once, so it has slightly different output from here
if feature.check("fsmonitor"):
    sh % "hg status" == "skipping invalid utf-8 filename: '\x9d\xc8\xac\xde\xa1\xee'"
    sh % "hg addremove" == ""
    sh % "hg commit -m 'adding a filename that is invalid utf8'" == r"""
        nothing changed
        [1]"""
else:
    # This is different from the fsmonitor output above because the Rust walker error
    # reporting escapes the invalid unicode characters with unicode codepoint \ufffd
    # (which encodes to bytes \xef\xbf\xbd).
    sh % "hg status" == "skipping invalid utf-8 filename: '\xef\xbf\xbd\xc8\xac\xde\xa1\xef\xbf\xbd'"
    sh % "hg addremove" == "skipping invalid utf-8 filename: '\xef\xbf\xbd\xc8\xac\xde\xa1\xef\xbf\xbd'"
    sh % "hg commit -m 'adding a filename that is invalid utf8'" == r"""
        skipping invalid utf-8 filename: '�Ȭޡ�'
        skipping invalid utf-8 filename: '�Ȭޡ�'
        nothing changed
        [1]"""
