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
smartlog=
remotenames=
"""
    >> "$HGRCPATH"
)

sh % "newclientrepo repo"

sh % "echo x" > "x"
sh % "hg commit -qAm x1"
sh % "hg book master1"
sh % "echo x" >> "x"
sh % "hg commit -qAm x2"
sh % "hg push -r . -q --to master1 --create"

# Non-bookmarked public heads should not be visible in smartlog

sh % "newclientrepo client test:repo_server master1" == ""
sh % "hg book mybook -r 'desc(x1)'"
sh % "hg up 'desc(x1)'" == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "hg smartlog -T '{desc} {bookmarks} {remotebookmarks}'" == r"""
    o  x2  remote/master1
    │
    @  x1 mybook"""

# Old head (rev 1) is still visible

sh % "echo z" >> "x"
sh % "hg commit -qAm x3"
sh % "hg push --non-forward-move -q --to master1"
sh % "hg smartlog -T '{desc} {bookmarks} {remotebookmarks}'" == r"""
    @  x3  remote/master1
    │
    o  x1 mybook"""

# Test configuration of "interesting" bookmarks

sh % "hg up -q '.^'"
sh % "echo x" >> "x"
sh % "hg commit -qAm x4"
sh % "hg push -q --to project/bookmark --create"
sh % "hg smartlog -T '{desc} {bookmarks} {remotebookmarks}'" == r"""
    o  x3  remote/master1
    │
    │ @  x4
    ├─╯
    o  x1 mybook"""

sh % "hg up '.^'" == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "hg smartlog -T '{desc} {bookmarks} {remotebookmarks}'" == r"""
    o  x3  remote/master1
    │
    │ o  x4
    ├─╯
    @  x1 mybook"""
(
    sh % "cat"
    << r"""
[smartlog]
repos=default/
names=project/bookmark
"""
    >> "$HGRCPATH"
)
sh % "hg smartlog -T '{desc} {bookmarks} {remotebookmarks}'" == r"""
    o  x3  remote/master1
    │
    │ o  x4
    ├─╯
    @  x1 mybook"""
(
    sh % "cat"
    << r"""
[smartlog]
names=master project/bookmark
"""
    >> "$HGRCPATH"
)
sh % "hg smartlog -T '{desc} {bookmarks} {remotebookmarks}'" == r"""
    o  x3  remote/master1
    │
    │ o  x4
    ├─╯
    @  x1 mybook"""
