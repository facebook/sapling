# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "setconfig 'extensions.treemanifest=!'"
sh % "cat" << r"""
[extensions]
smartlog=
remotenames=
""" >> "$HGRCPATH"

sh % "hg init repo"
sh % "cd repo"

sh % "echo x" > "x"
sh % "hg commit -qAm x"
sh % "hg book master"
sh % "echo x" >> "x"
sh % "hg commit -qAm x2"

# Non-bookmarked public heads should not be visible in smartlog

sh % "cd .."
sh % "hg clone repo client" == r"""
    updating to branch default
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "cd client"
sh % "hg book mybook -r 0"
sh % "hg up 0" == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "hg smartlog -T '{rev} {bookmarks} {remotebookmarks}'" == r"""
    o  1  default/master
    |
    @  0 mybook"""
# Old head (rev 1) should no longer be visible

sh % "echo z" >> "x"
sh % "hg commit -qAm x3"
sh % "hg push --non-forward-move -q --to master"
sh % "hg smartlog -T '{rev} {bookmarks} {remotebookmarks}'" == r"""
    @  2  default/master
    |
    o  0 mybook"""

# Test configuration of "interesting" bookmarks

sh % "hg up -q '.^'"
sh % "echo x" >> "x"
sh % "hg commit -qAm x4"
sh % "hg push -q --to project/bookmark --create"
sh % "hg smartlog -T '{rev} {bookmarks} {remotebookmarks}'" == r"""
    o  2  default/master
    |
    | @  3  default/project/bookmark
    |/
    o  0 mybook"""

sh % "hg up '.^'" == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "hg smartlog -T '{rev} {bookmarks} {remotebookmarks}'" == r"""
    o  2  default/master
    |
    @  0 mybook"""
sh % "cat" << r"""
[smartlog]
repos=default/
names=project/bookmark
""" >> "$HGRCPATH"
sh % "hg smartlog -T '{rev} {bookmarks} {remotebookmarks}'" == r"""
    o  3  default/project/bookmark
    |
    @  0 mybook"""
sh % "cat" << r"""
[smartlog]
names=master project/bookmark
""" >> "$HGRCPATH"
sh % "hg smartlog -T '{rev} {bookmarks} {remotebookmarks}'" == r"""
    o  2  default/master
    |
    | o  3  default/project/bookmark
    |/
    @  0 mybook"""

# Change remote master to a non-existing locally commit.

sh % "cat" << r"""
6dd477f76ca35ae46e82972648735867cf335d3e bookmarks remote/master
""" > ".hg/store/remotenames"
sh % "hg sl" == r"""
    o  changeset:   3:8e2253d950ae
    |  tag:         tip
    |  parent:      0:b292c1e3311f
    |  user:        test
    |  date:        Thu Jan 01 00:00:00 1970 +0000
    |  summary:     x4
    |
    @  changeset:   0:b292c1e3311f
       bookmark:    mybook
       user:        test
       date:        Thu Jan 01 00:00:00 1970 +0000
       summary:     x"""
