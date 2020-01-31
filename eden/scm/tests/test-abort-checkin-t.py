# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "cat" << r"""
from edenscm.mercurial import error
def hook(**args):
    raise error.Abort("no commits allowed")
def reposetup(ui, repo):
    repo.ui.setconfig("hooks", "pretxncommit.nocommits", hook)
""" > "abortcommit.py"
sh % "'abspath=`pwd`/abortcommit.py'"

sh % "cat" << r"""
[extensions]
mq =
abortcommit = $abspath
""" >> "$HGRCPATH"

sh % "hg init foo"
sh % "cd foo"
sh % "echo foo" > "foo"
sh % "hg add foo"

# mq may keep a reference to the repository so __del__ will not be
# called and .hg/journal.dirstate will not be deleted:

sh % "hg ci -m foo" == r"""
    error: pretxncommit.nocommits hook failed: no commits allowed
    transaction abort!
    rollback completed
    abort: no commits allowed
    [255]"""
sh % "hg ci -m foo" == r"""
    error: pretxncommit.nocommits hook failed: no commits allowed
    transaction abort!
    rollback completed
    abort: no commits allowed
    [255]"""

sh % "cd .."
