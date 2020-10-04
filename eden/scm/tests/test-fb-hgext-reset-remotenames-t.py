# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "setconfig 'extensions.treemanifest=!'"
sh % "cat" << r"""
[extensions]
reset=
remotenames=
""" >> "$HGRCPATH"

sh % "hg init repo"
sh % "cd repo"

sh % "echo x" > "x"
sh % "hg commit -qAm x"
sh % "hg book foo"
sh % "echo x" >> "x"
sh % "hg commit -qAm x2"

# Resetting past a remote bookmark should not delete the remote bookmark

sh % "cd .."
sh % "hg clone repo client" == r"""
    updating to branch default
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "cd client"
sh % "hg book bar"
sh % "hg reset --clean 'default/foo^'"
sh % "hg log -G -T '{node|short} {bookmarks} {remotebookmarks}\\n'" == r"""
    o  a89d614e2364  default/foo
    |
    @  b292c1e3311f bar"""
