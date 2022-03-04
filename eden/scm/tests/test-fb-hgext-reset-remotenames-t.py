# coding=utf-8
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "configure modernclient"
(
    sh % "cat"
    << r"""
[extensions]
reset=
remotenames=
"""
    >> "$HGRCPATH"
)

sh % "newclientrepo repo"

sh % "echo x" > "x"
sh % "hg commit -qAm x"
sh % "hg book foo"
sh % "echo x" >> "x"
sh % "hg commit -qAm x2"
sh % "hg push -q -r . --to foo --create"

# Resetting past a remote bookmark should not delete the remote bookmark

sh % "newclientrepo client test:repo_server foo"
sh % "hg book --list-remote *"
sh % "hg book bar"
sh % "hg reset --clean 'remote/foo^'"
sh % "hg log -G -T '{node|short} {bookmarks} {remotebookmarks}\\n'" == r"""
    o  a89d614e2364  remote/foo
    │
    @  b292c1e3311f bar"""
