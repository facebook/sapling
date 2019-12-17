# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


# https://bz.mercurial-scm.org/2137

# Setup:

# create a little extension that has 3 side-effects:
# 1) ensure changelog data is not inlined
# 2) make revlog to use lazyparser
# 3) test that repo.lookup() works
# 1 and 2 are preconditions for the bug; 3 is the bug.

sh % "cat" << r"""
from edenscm.mercurial import extensions, node, revlog

def reposetup(ui, repo):
    class wraprepo(repo.__class__):
        def commit(self, *args, **kwargs):
            result = super(wraprepo, self).commit(*args, **kwargs)
            tip1 = node.short(repo.changelog.tip())
            tip2 = node.short(repo.lookup(tip1))
            assert tip1 == tip2
            ui.write('new tip: %s\n' % tip1)
            return result
    repo.__class__ = wraprepo

def extsetup(ui):
    revlog._maxinline = 8             # split out 00changelog.d early
    revlog._prereadsize = 8           # use revlog.lazyparser
""" > "commitwrapper.py"

sh % "cat" << r"""
[extensions]
commitwrapper = `pwd`/commitwrapper.py
""" >> "$HGRCPATH"

sh % "hg init repo1"
sh % "cd repo1"
sh % "echo a" > "a"
sh % "hg commit -A '-madd a with a long commit message to make the changelog a bit bigger'" == r"""
    adding a
    new tip: 553596fad57b"""

# Test that new changesets are visible to repo.lookup():

sh % "echo a" >> "a"
sh % "hg commit '-mone more commit to demonstrate the bug'" == "new tip: 799ae3599e0e"

sh % "hg tip" == r"""
    changeset:   1:799ae3599e0e
    user:        test
    date:        Thu Jan 01 00:00:00 1970 +0000
    summary:     one more commit to demonstrate the bug"""

sh % "cd .."
