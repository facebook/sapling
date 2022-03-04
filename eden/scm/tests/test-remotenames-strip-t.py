# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "setconfig experimental.allowfilepeer=True"
sh % "enable remotenames"

# Test that hg debugstrip -B stops at remotenames
sh % "hg init server"
sh % "hg clone -q server client"
sh % "cd client"
sh % "echo x" > "x"
sh % "hg commit -Aqm a"
sh % "echo a" > "a"
sh % "hg commit -Aqm aa"
sh % "hg debugmakepublic"
sh % "hg push -q --to master --create"
sh % "echo b" > "b"
sh % "hg commit -Aqm bb"
sh % "hg book foo"
sh % "hg log -T '{desc} ({bookmarks}) ({remotebookmarks})\\n'" == r"""
    bb (foo) ()
    aa () (default/master public/a6e72781733c178cd290a07022bb6c8460749e7b)
    a () ()"""
sh % "hg debugstrip -qB foo" == "bookmark 'foo' deleted"
sh % "hg log -T '{desc} ({bookmarks}) ({remotebookmarks})\\n'" == r"""
    aa () (default/master public/a6e72781733c178cd290a07022bb6c8460749e7b)
    a () ()"""

# Test that hg debugstrip -B deletes bookmark even if there is a remote bookmark,
# but doesn't delete the commit.
sh % "hg init server"
sh % "hg clone -q server client"
sh % "cd client"
sh % "echo x" > "x"
sh % "hg commit -Aqm a"
sh % "hg debugmakepublic"
sh % "hg push -q --to master --create"
sh % "hg book foo"
sh % "hg log -T '{desc} ({bookmarks}) ({remotebookmarks})\\n'" == "a (foo) (default/master public/770eb8fce608e2c55f853a8a5ea328b659d70616)"
sh % "hg debugstrip -qB foo" == r"""
    bookmark 'foo' deleted
    abort: empty revision set
    [255]"""
sh % "hg log -T '{desc} ({bookmarks}) ({remotebookmarks})\\n'" == "a () (default/master public/770eb8fce608e2c55f853a8a5ea328b659d70616)"
