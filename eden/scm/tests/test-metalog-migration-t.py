# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

from testutil.autofix import eq
from testutil.dott import feature, sh, testtmp  # noqa: F401


def backup():
    """Backup .hg/store/{bookmarks,remotenames}"""
    for name in ["bookmarks", "remotenames"]:
        path = ".hg/store/%s" % name
        sh.cp(path, "%s.bak" % path)


def restore():
    """Rewrite .hg/store/{bookmarks,remotenames} with backup"""
    for name in ["bookmarks", "remotenames"]:
        path = ".hg/store/%s" % name
        sh.cp("%s.bak" % path, path)


def setbookmarks(name):
    """Set bookmarks to specified commit"""
    sh.hg("bookmark", "book", "-r", "desc(%s)" % name)
    sh.hg("debugremotebookmark", "remotebook", "desc(%s)" % name)


def listbookmarks():
    """List local and remote bookmarks"""
    local = sh.hg("log", "-r", sh.hg("bookmarks", "-T", "{node}"), "-T{desc}")
    remote = sh.hg(
        "log", "-r", sh.hg("bookmarks", "--remote", "-T", "{node}"), "-T{desc}"
    )
    return [local, remote]


sh.newrepo()
sh.setconfig("experimental.metalog=0")
sh.enable("remotenames")

sh % "drawdag" << r"""
C
|
B
|
A
"""

# Prepare bookmarks and remotenames. Set them to A in backup, and B on disk.

setbookmarks("A")
backup()
setbookmarks("B")

# Test migrating from disk to metalog.
# They should migrate "B" from disk to metalog and use it.

sh.setconfig("experimental.metalog=1")
eq(listbookmarks(), ["B", "B"])

# Metalog is the source of truth. Changes to .hg/store are ignored.

restore()
eq(listbookmarks(), ["B", "B"])

# Test migrating from metalog to disk.
# Metalog is not the source of truth. Changes to .hg/store are effective.

sh.setconfig("experimental.metalog=0")
setbookmarks("C")
eq(listbookmarks(), ["C", "C"])
restore()
eq(listbookmarks(), ["A", "A"])

# Migrate up again.
# At this time metalog should import "A" from disk to metalog, instead of
# using "B" that exists in metalog.

sh.setconfig("experimental.metalog=1")
eq(listbookmarks(), ["A", "A"])
