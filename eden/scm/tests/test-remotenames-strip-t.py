# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "enable remotenames"

# Test that hg debugstrip -B stops at remotenames
sh % "hg init server"
sh % "hg clone -q server client"
sh % "cd client"
sh % "echo x" > "x"
sh % "hg commit -Aqm a"
sh % "echo a" > "a"
sh % "hg commit -Aqm aa"
sh % "hg phase -p"
sh % "hg push -q --to master --create"
sh % "echo b" > "b"
sh % "hg commit -Aqm bb"
sh % "hg book foo"
sh % "hg log -T '{desc} ({bookmarks}) ({remotebookmarks})\\n'" == r"""
    bb (foo) ()
    aa () (default/master)
    a () ()"""
sh % "hg debugstrip -qB foo" == "bookmark 'foo' deleted"
sh % "hg log -T '{desc} ({bookmarks}) ({remotebookmarks})\\n'" == r"""
    aa () (default/master)
    a () ()"""

# Test that hg debugstrip -B deletes bookmark even if there is a remote bookmark,
# but doesn't delete the commit.
sh % "hg init server"
sh % "hg clone -q server client"
sh % "cd client"
sh % "echo x" > "x"
sh % "hg commit -Aqm a"
sh % "hg phase -p"
sh % "hg push -q --to master --create"
sh % "hg book foo"
sh % "hg log -T '{desc} ({bookmarks}) ({remotebookmarks})\\n'" == "a (foo) (default/master)"
sh % "hg debugstrip -qB foo" == r"""
    bookmark 'foo' deleted
    abort: empty revision set
    [255]"""
sh % "hg log -T '{desc} ({bookmarks}) ({remotebookmarks})\\n'" == "a () (default/master)"
